use std::{
    collections::VecDeque,
    error::Error,
    fmt::{Display, Write},
    sync::{Arc, Condvar, Mutex, Weak},
};

#[derive(Debug)]
struct SendError;

impl Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{:?}", self))
    }
}

impl Error for SendError {}

struct Sender<T> {
    shared: Weak<Shared<T>>,
}

impl<T> Sender<T> {
    fn send(&self, value: T) -> Result<(), SendError> {
        let share = match self.shared.upgrade() {
            Some(share) => share,
            None => panic!("Sender send value but the Receiver has closed."),
        };
        share.queue.lock().unwrap().push_back(value);
        share.avaliable.notify_one();
        Ok(())
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Sender {
            shared: Weak::clone(&self.shared),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        if Weak::weak_count(&self.shared) == 1 {
            self.shared.upgrade().unwrap().avaliable.notify_one();
        }
    }
}

struct Receiver<T> {
    shared: Arc<Shared<T>>,
    buffer: VecDeque<T>,
}

impl<T> Receiver<T> {
    fn recv(&mut self) -> Option<T> {
        if let v @ Some(_) = self.buffer.pop_front() {
            return v;
        }
        let mut queue = self.shared.queue.lock().unwrap();
        loop {
            match queue.pop_front() {
                v @ Some(_) => {
                    std::mem::swap(&mut self.buffer, &mut queue);
                    return v;
                }
                None if Arc::weak_count(&self.shared) == 0 => return None,
                None => {
                    queue = self.shared.avaliable.wait(queue).unwrap();
                }
            }
        }
    }
}

struct Shared<T> {
    queue: Mutex<VecDeque<T>>,
    avaliable: Condvar,
}

fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Shared {
        queue: Mutex::default(),
        avaliable: Condvar::default(),
    });
    (
        Sender {
            shared: Arc::downgrade(&shared),
        },
        Receiver {
            shared,
            buffer: VecDeque::default(),
        },
    )
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn test_rx_tx() -> anyhow::Result<()> {
        let (tx, mut rx) = channel();
        let _ = tx.send(1);
        assert_eq!(rx.recv(), Some(1));
        Ok(())
    }

    #[test]
    fn test_rx_tx_multi_threads() -> anyhow::Result<()> {
        let (tx, mut rx) = channel();
        const CYCLE: usize = 1000000;
        for _ in 0..CYCLE {
            let tx = tx.clone();
            rayon::spawn(move || {
                let _ = tx.send(1);
            });
        }
        let jh = std::thread::spawn(move || {
            let mut count = 0;
            while let Some(v) = rx.recv() {
                count += v;
            }
            assert_eq!(count, CYCLE);
        });
        drop(tx);
        let _ = jh.join();
        Ok(())
    }

    #[test]
    fn test_drop_rx() {
        let (tx, mut rx) = channel::<i32>();
        drop(tx);
        let _ = rx.recv();
    }

    #[test]
    #[should_panic]
    fn test_drop_tx() {
        let (tx, rx) = channel::<i32>();
        drop(rx);
        let _ = tx.send(1);
    }
}
