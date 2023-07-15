use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
};

struct Sender<T> {
    shared: Arc<Shared<T>>,
}

impl<T> Sender<T> {
    fn send(&self, value: T) -> anyhow::Result<()> {
        self.shared.inner.lock().unwrap().queue.push_back(value);
        self.shared.avaliable.notify_one();
        Ok(())
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        let mut inner = self.shared.inner.lock().unwrap();
        inner.tx_count += 1;
        drop(inner);
        Sender {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        eprintln!("drop Sender");
        let mut inner = self.shared.inner.lock().unwrap();
        eprintln!("drop Sender2");
        inner.tx_count -= 1;
        eprintln!("decrease tx_count");
        if inner.tx_count == 0 {
            self.shared.avaliable.notify_one();
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
        let mut inner = self.shared.inner.lock().unwrap();
        loop {
            match inner.queue.pop_front() {
                v @ Some(_) => {
                    std::mem::swap(&mut self.buffer, &mut inner.queue);
                    return v;
                }
                None if inner.tx_count == 0 => return None,
                None => {
                    inner = self.shared.avaliable.wait(inner).unwrap();
                }
            }
        }
    }
}

struct Shared<T> {
    inner: Mutex<Inner<T>>,
    avaliable: Condvar,
}

#[derive(Default)]
struct Inner<T> {
    queue: VecDeque<T>,
    tx_count: usize,
}

fn channel<T: Default>() -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Shared {
        inner: Mutex::new(Inner {
            queue: VecDeque::default(),
            tx_count: 1,
        }),
        avaliable: Condvar::default(),
    });
    (
        Sender {
            shared: Arc::clone(&shared),
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
