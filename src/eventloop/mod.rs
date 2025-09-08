use thiserror::Error;

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;

#[derive(Debug, Error)]
pub enum EventError {
    #[error("Event not found")]
    NotFound,
    #[error("Event execution failed")]
    ExecutionFailed,
    #[error("Execution Failed: {0}")]
    ExecutionFailedWithMessage(String),
}

#[derive(Debug, Error)]
pub enum EventLoopError {
    #[error("Event error: {0}")]
    EventError(EventError),
    #[error("Event loop is not running")]
    NotRunning,
    #[error("Mutex poisoned")]
    MutexPoisoned,
}

type EvResult = Result<(), EventError>;

pub trait BasicEventTrait: Fn(u64) -> EvResult + Send + Sync {}
impl<T> BasicEventTrait for T where T: Fn(u64) -> EvResult + Send + Sync {}

pub enum EventFnType {
    Basic(Box<dyn BasicEventTrait>),
    Stop,
}

impl std::fmt::Debug for EventFnType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventFnType::Basic(_) => write!(f, "Basic(<closure>) Event"),
            EventFnType::Stop => write!(f, "Stop Event"),
        }
    }
}

pub struct Event {
    pub time: u64,
    pub event_fn: EventFnType,
}

impl std::fmt::Debug for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Event")
            .field("time", &self.time)
            .field("event_fn", &self.event_fn)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct EventLoop {
    events: Arc<Mutex<VecDeque<Event>>>,
    current_time: Arc<RwLock<u64>>,
    running: Arc<RwLock<bool>>,
}

impl Default for EventLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLoop {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(VecDeque::new())),
            current_time: Arc::new(RwLock::new(0)),
            running: Arc::new(RwLock::new(false)),
        }
    }

    pub fn schedule_event<F>(&self, at: u64, event_fn: F)
    where
        F: BasicEventTrait + 'static,
    {
        let event = Event {
            time: at,
            event_fn: EventFnType::Basic(Box::new(event_fn)),
        };
        let mut events = self.events.lock().unwrap();
        events.push_back(event);
        events.make_contiguous().sort_by_key(|e| e.time);
    }

    pub fn run(&self) -> Result<(), EventLoopError> {
        let mut running = self.running.write().unwrap();
        *running = true;
        drop(running);

        while *self.running.read().unwrap() {
            // Pop off the next event
            let next_event_opt = {
                let mut events = self
                    .events
                    .lock()
                    .map_err(|_| EventLoopError::MutexPoisoned)?;
                events.pop_front()
            };

            // Return if no more events!
            let Some(event) = next_event_opt else {
                return Ok(());
            };

            // Update current time
            {
                let mut current_time = self
                    .current_time
                    .write()
                    .map_err(|_| EventLoopError::MutexPoisoned)?;
                *current_time = event.time;
            }
            // Execute the event
            match event.event_fn {
                EventFnType::Basic(f) => f(event.time).map_err(EventLoopError::EventError)?,
                EventFnType::Stop => {
                    let mut running = self.running.write().unwrap();
                    *running = false;
                }
            }
        }
        Ok(())
    }

    pub fn run_realtime(&self) -> Result<(), EventLoopError> {
        let mut running = self.running.write().unwrap();
        *running = true;
        drop(running);

        let start_instant = std::time::Instant::now();

        while *self.running.read().unwrap() {
            // Pop off the next event
            let next_event_opt = {
                let mut events = self
                    .events
                    .lock()
                    .map_err(|_| EventLoopError::MutexPoisoned)?;
                events.pop_front()
            };

            // Return if no more events!
            let Some(event) = next_event_opt else {
                return Ok(());
            };

            // Calculate the real time to wait until this event
            let wait_duration = {
                let elapsed = start_instant.elapsed().as_micros() as i64;
                std::cmp::max(0, event.time as i64 - elapsed) as u64
            };

            // Sleep until the event time
            if wait_duration > 0 {
                std::thread::sleep(std::time::Duration::from_micros(wait_duration));
            }

            // Update current time
            {
                let mut current_time = self
                    .current_time
                    .write()
                    .map_err(|_| EventLoopError::MutexPoisoned)?;
                *current_time = event.time;
            }

            // Execute the event
            match event.event_fn {
                EventFnType::Basic(f) => f(event.time).map_err(EventLoopError::EventError)?,
                EventFnType::Stop => {
                    let mut running = self.running.write().unwrap();
                    *running = false;
                }
            }
        }
        Ok(())
    }

    pub fn stop(&self) -> Result<(), EventLoopError> {
        let mut running = self.running.write().unwrap();
        if !*running {
            return Err(EventLoopError::NotRunning);
        }
        *running = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_loop_basic() {
        let event_loop = EventLoop::new();
        let counter = Arc::new(Mutex::new(0));
        let counter_clone = counter.clone();
        event_loop.schedule_event(100, move |_time| {
            let mut num = counter_clone.lock().unwrap();
            *num += 1;
            Ok(())
        });
        event_loop.run().unwrap();
        assert_eq!(*counter.lock().unwrap(), 1);
    }

    #[test]
    fn test_realtime_event_loop() {
        let event_loop = EventLoop::new();

        for secs in 0..3 {
            event_loop.schedule_event(secs * 10000, move |time| {
                let now = chrono::Utc::now();
                println!("Event at simulated time: {} us :: {}", time, now);
                use std::io::Write;
                std::io::stdout().flush().unwrap();
                Ok(())
            });
        }

        event_loop.run_realtime().unwrap();
    }
}
