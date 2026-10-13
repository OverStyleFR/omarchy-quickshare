//! The Nearby length-prefixed stream is identical on TCP and Bluetooth RFCOMM.
use anyhow::{bail, Result};
use std::fmt::Debug;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub trait SessionIo: AsyncRead + AsyncWrite + Unpin + Send + Debug {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Debug> SessionIo for T {}
pub type TransportStream = Box<dyn SessionIo>;
pub const MAX_FRAME: usize = 1024 * 1024;

#[derive(Debug)]
struct PendingFrame {
    prefix: [u8; 4],
    prefix_written: usize,
    body: Vec<u8>,
    body_written: usize,
}

#[derive(Debug, Default)]
pub(crate) struct FrameReader {
    prefix: [u8; 4],
    prefix_read: usize,
    body: Vec<u8>,
    body_read: usize,
}

impl FrameReader {
    // read(), unlike read_exact(), is cancellation-safe. Cursor state remains
    // attached to the channel while a consent/cancel event wins a select!.
    pub(crate) async fn read_frame<T: AsyncRead + Unpin + ?Sized>(
        &mut self,
        stream: &mut T,
    ) -> Result<Vec<u8>> {
        while self.prefix_read < 4 {
            let n = stream.read(&mut self.prefix[self.prefix_read..]).await?;
            if n == 0 {
                bail!("peer closed transport");
            }
            self.prefix_read += n;
        }
        if self.body.is_empty() {
            let size = u32::from_be_bytes(self.prefix) as usize;
            if size == 0 || size > MAX_FRAME {
                bail!("invalid Nearby frame size: {size}");
            }
            self.body.resize(size, 0);
        }
        while self.body_read < self.body.len() {
            let n = stream.read(&mut self.body[self.body_read..]).await?;
            if n == 0 {
                bail!("peer closed mid-frame");
            }
            self.body_read += n;
        }
        self.prefix_read = 0;
        self.body_read = 0;
        Ok(std::mem::take(&mut self.body))
    }
}

#[derive(Debug)]
pub struct FramedStream {
    pub stream: TransportStream,
    reader: FrameReader,
    pending_write: Option<PendingFrame>,
}

impl FramedStream {
    pub fn new(stream: TransportStream) -> Self {
        Self {
            stream,
            reader: FrameReader::default(),
            pending_write: None,
        }
    }

    pub async fn read_frame(&mut self) -> Result<Vec<u8>> {
        self.reader.read_frame(&mut self.stream).await
    }

    /// Retains the frame before a cancellation select can skip polling its
    /// writer. This also preserves the ciphertext's already-assigned sequence.
    pub fn queue_frame(&mut self, bytes: Vec<u8>) -> Result<()> {
        if self.pending_write.is_some() {
            bail!("outbound frame is still pending");
        }
        if bytes.is_empty() || bytes.len() > MAX_FRAME {
            bail!("outbound frame exceeds limit");
        }
        self.pending_write = Some(PendingFrame {
            prefix: (bytes.len() as u32).to_be_bytes(),
            prefix_written: 0,
            body: bytes,
            body_written: 0,
        });
        Ok(())
    }

    pub fn has_pending_frame(&self) -> bool {
        self.pending_write.is_some()
    }

    /// Starts an owned frame. Callers selecting against cancellation must queue
    /// synchronously first and then select write_queued_frame().
    pub async fn write_frame(&mut self, bytes: Vec<u8>) -> Result<()> {
        self.queue_frame(bytes)?;
        self.write_queued_frame().await
    }

    pub async fn write_queued_frame(&mut self) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(30), self.finish_pending_frame()).await??;
        Ok(())
    }

    /// Completes exactly the retained frame, including its flush, without a
    /// deadline. Cancellation and errors retain only the unwritten suffix;
    /// even a flush failure keeps the frame pending, without rewriting bytes.
    /// A fatal transport error may make completion impossible: do not append
    /// another frame to that transport unless this method succeeds.
    pub async fn finish_pending_frame(&mut self) -> Result<()> {
        let Some(pending) = self.pending_write.as_mut() else {
            return Ok(());
        };
        while pending.prefix_written < pending.prefix.len() {
            let n = self
                .stream
                .write(&pending.prefix[pending.prefix_written..])
                .await?;
            if n == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::WriteZero).into());
            }
            pending.prefix_written += n;
        }
        while pending.body_written < pending.body.len() {
            let n = self
                .stream
                .write(&pending.body[pending.body_written..])
                .await?;
            if n == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::WriteZero).into());
            }
            pending.body_written += n;
        }
        // Keeping Some after both cursors reach the end represents a pending
        // flush. Retrying it never emits the prefix or body a second time.
        self.stream.flush().await?;
        self.pending_write = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::{poll_fn, Future};
    use std::pin::Pin;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::task::{Context, Poll};
    use tokio::io::{DuplexStream, ReadBuf};

    #[tokio::test]
    async fn canceled_first_frame_reads_preserve_consecutive_frames_at_handoff() {
        let (mut stream, mut peer) = tokio::io::duplex(64);
        let mut reader = FrameReader::default();
        let first: Vec<u8> = (0..19).collect();
        let second = vec![91, 92, 93];
        let prefix = (first.len() as u32).to_be_bytes();

        peer.write_all(&prefix[..2]).await.unwrap();
        {
            let interrupted = reader.read_frame(&mut stream);
            tokio::pin!(interrupted);
            poll_fn(|cx| {
                assert!(interrupted.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
        }

        peer.write_all(&prefix[2..]).await.unwrap();
        peer.write_all(&first[..7]).await.unwrap();
        {
            let interrupted = reader.read_frame(&mut stream);
            tokio::pin!(interrupted);
            poll_fn(|cx| {
                assert!(interrupted.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
        }

        peer.write_all(&first[7..]).await.unwrap();
        peer.write_all(&(second.len() as u32).to_be_bytes())
            .await
            .unwrap();
        peer.write_all(&second).await.unwrap();
        assert_eq!(reader.read_frame(&mut stream).await.unwrap(), first);

        // Classification consumes only its first frame. A fresh session reader
        // must receive the next frame intact from the same, unboxed stream.
        let mut session = FramedStream::new(Box::new(stream));
        assert_eq!(session.read_frame().await.unwrap(), second);
        let mut extra = [0];
        let trailing = session.stream.read(&mut extra);
        tokio::pin!(trailing);
        poll_fn(|cx| {
            assert!(trailing.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
    }

    #[tokio::test]
    async fn abandoned_writes_resume_exact_frames_under_backpressure() {
        // A single poll fills the duplex buffer and parks in the prefix (2)
        // or body (7), without timers or dependence on task scheduling.
        for capacity in [2, 7] {
            let (a, b) = tokio::io::duplex(capacity);
            let mut writer = FramedStream::new(Box::new(a));
            let mut reader = FramedStream::new(Box::new(b));
            let first: Vec<u8> = (0..19).collect();
            let second = vec![91, 92, 93];
            {
                let interrupted = writer.write_frame(first.clone());
                tokio::pin!(interrupted);
                poll_fn(|cx| {
                    assert!(interrupted.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
            }
            assert!(writer.write_frame(vec![99]).await.is_err());
            let (sent, received) = tokio::time::timeout(Duration::from_secs(1), async {
                tokio::join!(
                    async {
                        writer.finish_pending_frame().await?;
                        writer.write_frame(second.clone()).await
                    },
                    async {
                        let first = reader.read_frame().await?;
                        let second = reader.read_frame().await?;
                        Ok::<_, anyhow::Error>((first, second))
                    }
                )
            })
            .await
            .unwrap();
            sent.unwrap();
            assert_eq!(received.unwrap(), (first, second));
            let mut extra = [0];
            let trailing = reader.stream.read(&mut extra);
            tokio::pin!(trailing);
            poll_fn(|cx| {
                assert!(trailing.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
        }
    }

    #[derive(Debug)]
    struct FlushGate {
        stream: DuplexStream,
        ready: Arc<AtomicBool>,
    }
    impl AsyncRead for FlushGate {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buffer: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.stream).poll_read(cx, buffer)
        }
    }
    impl AsyncWrite for FlushGate {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Pin::new(&mut self.stream).poll_write(cx, bytes)
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            if self.ready.load(Ordering::Relaxed) {
                Pin::new(&mut self.stream).poll_flush(cx)
            } else {
                Poll::Pending
            }
        }
        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.stream).poll_shutdown(cx)
        }
    }

    #[tokio::test]
    async fn abandoned_flush_does_not_rewrite_completed_frame() {
        let (a, b) = tokio::io::duplex(64);
        let ready = Arc::new(AtomicBool::new(false));
        let mut writer = FramedStream::new(Box::new(FlushGate {
            stream: a,
            ready: ready.clone(),
        }));
        let mut reader = FramedStream::new(Box::new(b));
        {
            let interrupted = writer.write_frame(vec![1, 2, 3]);
            tokio::pin!(interrupted);
            poll_fn(|cx| {
                assert!(interrupted.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
        }
        assert_eq!(reader.read_frame().await.unwrap(), vec![1, 2, 3]);
        assert!(writer.write_frame(vec![99]).await.is_err());
        ready.store(true, Ordering::Relaxed);
        writer.finish_pending_frame().await.unwrap();
        writer.write_frame(vec![4, 5]).await.unwrap();
        assert_eq!(reader.read_frame().await.unwrap(), vec![4, 5]);
    }

    #[tokio::test]
    async fn framing_survives_interrupted_prefix_and_rejects_oversize() {
        let (a, mut b) = tokio::io::duplex(64);
        let mut reader = FramedStream::new(Box::new(a));
        b.write_all(&[0, 0]).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(5), reader.read_frame())
                .await
                .is_err()
        );
        b.write_all(&[0, 3, 1, 2, 3]).await.unwrap();
        assert_eq!(reader.read_frame().await.unwrap(), vec![1, 2, 3]);
        b.write_all(&((MAX_FRAME + 1) as u32).to_be_bytes())
            .await
            .unwrap();
        assert!(reader.read_frame().await.is_err());
    }
}
