use async_std::task::spawn;
use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use futures::future::BoxFuture;
use async_channel::bounded;

thread_local! {
    static SIGNAL: Arc<Signal> = Arc::new(Signal::new());
    static RUNNABLE: Mutex<VecDeque<Arc<Task>>> = Mutex::new(VecDeque::with_capacity(1024));
}

enum State {
    Empty,
    Waiting,
    Notified,
}

struct Signal {
    state: Mutex<State>,
    cond: std::sync::Condvar,
}

impl Signal {
    fn new() -> Signal {
        Signal {
            state: Mutex::new(State::Notified),
            cond: std::sync::Condvar::new(),
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
                while *state == State::Waiting {
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
    future: Mutex<BoxFuture<'static, ()>>,
    signal: Arc<Signal>,
}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        RUNNABLE.with(|runnable| {
            runnable.lock().unwrap().push_back(self.clone());
        });
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

fn block_on<F: Future>(future: F) -> F::Output {
    let mut fut = std::pin::Pin::new(Box::pin(future));
    let signal = Arc::new(Signal::new());
    let waker = Waker::from(signal.clone());
    let mut cx = Context::from_waker(&waker);

    SIGNAL.with(|signal| {
        RUNNABLE.with(|runnable| {
            while let Poll::Pending = fut.as_mut().poll(&mut cx) {
                while let Some(task) = runnable.lock().unwrap().pop_front() {
                    let waker = Waker::from(task.clone());
                    let mut cx = Context::from_waker(&waker);
                    let _ = task.future.lock().unwrap().as_mut().poll(&mut cx);
                }
                signal.wait();
            }
        });
    })
}

async fn demo() {
    let (tx, rx) = bounded(1);
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
