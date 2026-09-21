use assuan_library::{Client, Command};

async fn invalid(client: &mut Client) {
  let mut transaction = client.command(Command::new("GETINFO", b"version").unwrap()).await.unwrap();
  let event = transaction.next().await.unwrap();
  let _next = transaction.next().await.unwrap();
  drop(event);
}

fn main() {}
