use assuan_library::{Client, Command};

async fn invalid(client: &mut Client) {
  let mut transaction = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  let _second = client.command(Command::new("NOP", b"").unwrap()).await.unwrap();
  let _event = transaction.next().await.unwrap();
}

fn main() {}
