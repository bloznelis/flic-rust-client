use tokio::io::*;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use super::commands::stream_mapper::CommandToByteMapper;
use super::commands::Command;
use super::events::stream_mapper::*;
use super::events::Event;

pub fn event_handler<F>(f: F) -> EventClosureMutex
where
    F: FnMut(&Event) + Sync + Send + 'static,
{
    Box::new(f)
}

type EventClosure = dyn FnMut(&Event) + Sync + Send + 'static;
type EventClosureMutex = Box<EventClosure>;

pub struct FlicClient {
    reader: Mutex<OwnedReadHalf>,
    writer: Mutex<OwnedWriteHalf>,
    is_running: Mutex<bool>,
    command_mapper: Mutex<CommandToByteMapper>,
    event_mapper: Mutex<ByteToEventMapper>,
    handlers: Mutex<Vec<EventClosureMutex>>,
}

impl FlicClient {
    pub async fn new(conn: &str) -> Result<FlicClient> {
        TcpStream::connect(conn)
            .await
            .map(|s| s.into_split())
            .map(|(reader, writer)| FlicClient {
                reader: Mutex::new(reader),
                writer: Mutex::new(writer),
                is_running: Mutex::new(true),
                command_mapper: Mutex::new(CommandToByteMapper::new()),
                event_mapper: Mutex::new(ByteToEventMapper::new()),
                handlers: Mutex::new(vec![]),
            })
    }
    pub async fn register_event_handler(self, event: EventClosureMutex) -> Self {
        self.handlers.lock().await.push(event);
        self
    }

    pub async fn listen(&self) {
        while *self.is_running.lock().await {
            let mut reader = self.reader.lock().await;
            let mut buffer = vec![0; 2048];

            let size = reader.read(&mut buffer).await;

            match size {
                Ok(size) if size > 0 => {
                    buffer.truncate(size);
                    for b in buffer.iter() {
                        match self.event_mapper.lock().await.map(*b) {
                            EventResult::Some(Event::NoOp) => { }
                            EventResult::Some(event) => {
                                self.handlers
                                    .lock()
                                    .await
                                    .iter_mut()
                                    .for_each(|handler| handler(&event))
                            }
                            _ => {}
                        }
                    }
                }
                Ok(_) => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
                Err(e) => {
                    eprintln!("Error reading from reader: {}", e);
                    break;
                }
            }
        }
    }

    pub async fn stop(&self) {
        *self.is_running.lock().await = false;
    }

    pub async fn submit(&self, cmd: Command) {
        let mut writer = self.writer.lock().await;
        for b in self.command_mapper.lock().await.map(cmd) {
            let _ = writer.write_u8(b).await;
        }
    }
}
