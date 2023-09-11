use std::cell::RefCell;
use std::sync::Condvar;
use async_std::task::spawn;
use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker, Wake};
use futures::future::BoxFuture;

struct Signal {
    state: Mutex<State>,
    cond: Condvar,
}

enum State {
    Empty,
    Waiting,
    Notified,
}

impl Signal {
    fn new() -> Signal {
        Signal { 
            state: Mutex::new(State::Notified), 
            cond: Condvar::new() 
        }
    }
    fn wait(&self) {
        let mut state = self.state.lock().unwrap();
        match *state {
            State::Notified => *state = State::Empty,
            State::Waiting => {
                panic!("multiple wait");
            }
            State::Empty => {
                *state = State::Waiting;
                while let State::Waiting = *state {
                    state = self.cond.wait(state).unwrap();
                }
            }
        }
    }
    fn notify(&self) {
        let mut state = self.state.lock().unwrap();
        match *state {
            State::Notified => {}
            State::Empty => *state = State::Notified,
            State::Waiting => {
                *state = State::Empty;
                self.cond.notify_one();
            }
        }
    }
}

impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.notify();
    }
}

struct Task {
    future: RefCell<BoxFuture<'static, ()>>,
    signal: Arc<Signal>,
}

unsafe impl Send for Task {}
unsafe impl Sync for Task {}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        RUNNABLE.with(|runnable| runnable.lock().unwrap().push_back(self.clone()));
        self.signal.notify();
    }
}

struct Demo;

impl Future for Demo {
    type Output = ();
    fn poll(self: std::pin::Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        println!("hello world");
        Poll::Ready(())
    }
}

scoped_tls::scoped_thread_local!(static SIGNAL: Arc<Signal>);
scoped_tls::scoped_thread_local!(static RUNNABLE: Mutex<VecDeque<Arc<Task>>>);

fn block_on<F: Future>(future: F) -> F::Output {
    let mut fut = std::pin::pin!(future);
    let signal = Arc::new(Signal::new());
    let waker = Waker::from(signal.clone());

    let mut cx = Context::from_waker(&waker);
    let runnable: Mutex<VecDeque<Arc<Task>>> = Mutex::new(VecDeque::with_capacity(1024));
    SIGNAL.set(&signal, || {
        RUNNABLE.set(&runnable, || {
            loop {
                if let Poll::Ready(output) = fut.as_mut().poll(&mut cx) {
                    return output;
                }
                while let Some(task) = runnable.lock().unwrap().pop_front() {
                    let waker = Waker::from(task.clone());
                    let mut cx = Context::from_waker(&waker);
                    let _ = task.future.borrow_mut().as_mut().poll(&mut cx);
                }
                signal.wait();
            }
        })
    })
}

async fn demo() {
    let (tx, rx) = async_channel::bounded(1);
    spawn(demo2(tx));
    println!("hello world!");
    let _ = rx.recv().await;
}

async fn demo2(tx: async_channel::Sender<()>) {
    println!("hello world2!");
    let _ = tx.send(()).await;
}

fn main() {
    block_on(demo());
}
