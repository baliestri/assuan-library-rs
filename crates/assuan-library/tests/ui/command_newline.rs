fn main() {
  let _ = assuan_library::command!("GETINFO\nversion");
}
