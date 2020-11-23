use std::sync::{Arc, Mutex};
use std::pin::Pin;
use futures::task::{Context, Poll};
use futures::FutureExt;
use futures::{Future, Stream};
use futures::task::AtomicWaker;

pub fn msg_stream() -> MsgStreamHandle {
    let stream = Arc::new(Mutex::new(MsgStream::new()));
    let handle = MsgStreamHandle { inner: stream.clone() };
    return handle;
}

pub struct MsgStream {
    messages: Vec<Arc<Vec<u8>>>,
    wakers: Vec<(usize, Arc<AtomicWaker>)>,
    reader_counter: usize,
}

impl MsgStream {
    fn new() -> Self {
        MsgStream {
            messages: Vec::new(),
            wakers: Vec::new(),
            reader_counter: 0,
        }
    }

    fn append(&mut self, msg: Vec<u8>) {
        self.messages.push(Arc::new(msg));
        for (_reader_id, waker) in self.wakers.iter() {
            waker.wake();
        }
    }
}

#[derive(Clone)]
pub struct MsgStreamHandle {
    inner: Arc<Mutex<MsgStream>>,
}

impl MsgStreamHandle {
    pub fn reader(&self) -> MsgStreamReader {
        let mut inner = self.inner.lock().unwrap();

        let reader_id = inner.reader_counter;
        inner.reader_counter += 1;

        let waker = Arc::new(AtomicWaker::new());
        inner.wakers.push((reader_id, waker.clone()));
    
        MsgStreamReader {
            stream: self.clone(),
            reader_id,
            waker,
            pos: 0,
        }
    }

    pub fn write(&mut self, msg: Vec<u8>) {
        let mut inner = self.inner.lock().unwrap();
        inner.append(msg);
    }
}

pub struct MsgStreamReader {
    stream: MsgStreamHandle,
    waker: Arc<AtomicWaker>,
    reader_id: usize,
    pos: usize,
}

impl MsgStreamReader {
    pub fn recv<'a>(&'a mut self) -> Recv<'a> {
        Recv {
            reader: self,
        }
    }

    pub fn reset_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    pub fn clone(&self) -> Self {
        let mut r = self.stream.reader();
        r.pos = self.pos;
        return r;
    }
}

impl Drop for MsgStreamReader {
    
    fn drop(&mut self) {
        let mut inner = self.stream.inner.lock().unwrap();
        inner.wakers.retain(|(id, _)| id != &self.reader_id);
    }
}

impl Stream for MsgStreamReader {
    type Item = Arc<Vec<u8>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>)
        -> Poll<Option<Self::Item>>
    {
        self.recv().poll_unpin(cx).map(|item| Some(item))
    }
}

pub struct Recv<'s> {
    reader: &'s mut MsgStreamReader,
}

impl<'s> Future for Recv<'s> {
    type Output = Arc<Vec<u8>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>
    {
        let Recv { reader } = self.get_mut();
    
        let inner = reader.stream.inner.lock().unwrap();
        if inner.messages.len() > reader.pos {
            let value = inner.messages[reader.pos].clone();
            reader.pos += 1;
            Poll::Ready(value)
        } else {
            reader.waker.register(cx.waker());
            Poll::Pending
        }
    }
}
