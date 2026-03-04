use tokio::sync::mpsc::Receiver;

use redis::aio::MultiplexedConnection;
use tracing::{debug, info};

use super::handlers::*;
use crate::allocation::{
    adapters::repository::{Repository, SqlxRepository},
    domain::{commands::Command, events::Event},
};

#[derive(Debug)]
pub enum Message {
    Command(Command),
    Event(Event),
}

pub struct MessageBus<T: Repository> {
    pub repository: T,
    pub events: Vec<Event>,
    pub message_receiver: Receiver<Message>,
    pub redis_conn: MultiplexedConnection,
}

impl MessageBus<SqlxRepository> {
    pub fn new(
        repository: SqlxRepository,
        rx: Receiver<Message>,
        conn: MultiplexedConnection,
    ) -> Self {
        Self {
            repository,
            events: vec![],
            message_receiver: rx,
            redis_conn: conn,
        }
    }

    pub async fn init(mut self) {
        while let Some(msg) = self.message_receiver.recv().await {
            debug!("service_bus got message: {msg:?}");
            self.handle(msg).await
        }
    }

    async fn handle(&mut self, message: Message) {
        match message {
            Message::Event(event) => self.events.push(event),
            Message::Command(command) => self.handle_command(command).await,
        }

        loop {
            self.loop_events().await;
            self.events = self.repository.collect_new_events().await;
            if self.events.is_empty() {
                break;
            }
        }
    }

    async fn loop_events(&mut self) {
        for event in self.events.clone().iter() {
            self.handle_event(event).await;
        }
    }

    async fn handle_event(&mut self, event: &Event) {
        debug!("Event handler for: {event:?}");
        let _ = match event {
            Event::Allocated(allocated) => {
                let _ = add_allocation_to_read_model(allocated, &mut self.repository).await;
                publish_allocated_event(allocated, self.redis_conn.clone()).await
            }
            Event::Deallocated(deallocated) => {
                let _ = delete_allocation_to_read_model(deallocated, &mut self.repository).await;
                reallocate(deallocated, &mut self.repository).await
            }
            Event::OutOfStock(_out_of_stock) => {
                // send out of stock email notification.
                info!("Sending out of stock email notification.");
                async { Ok(()) }.await
            }
        };
    }

    async fn handle_command(&mut self, command: Command) {
        debug!("Command handler for: {command:?}");
        let _ = match command {
            Command::Allocate(alloc) => allocate(alloc, &mut self.repository).await,
            Command::CreateBatch(create_batch) => {
                add_batch(create_batch, &mut self.repository).await
            }
            Command::ChangeBatchQuantity(change_batch_quantity) => {
                change_batch_qty(change_batch_quantity, &mut self.repository).await
            }
        };

        let new_events = self.repository.collect_new_events().await;
        self.events.extend(new_events);
    }
}
