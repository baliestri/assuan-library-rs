use assuan_library::SecretBytes;

fn main() {
  let secret = SecretBytes::with_capacity(16).unwrap();
  let _copy = secret.clone();
}
