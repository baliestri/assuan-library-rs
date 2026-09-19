//! Remote errors are bounded wire content and diagnostic output is redacted.
use assuan_server::HandlerError;

#[test]
fn remote_errors_use_the_exact_encoded_limit() {
  for code in [0, 1, u32::MAX] {
    let budget = 1000 - "ERR ".len() - code.to_string().len() - 1 - 1;
    assert!(HandlerError::remote(code, &"x".repeat(budget)).is_ok());
    assert!(HandlerError::remote(code, &"x".repeat(budget + 1)).is_err());
    assert!(HandlerError::remote(code, "").is_ok());
  }
  assert!(HandlerError::remote(1, &"é".repeat(497)).is_err());
  for text in ["bad\rtext", "bad\ntext", "bad\0text"] {
    assert!(HandlerError::remote(1, text).is_err());
    let unchecked = HandlerError::Remote {
      code: 1,
      message: text.to_owned(),
    };
    assert!(unchecked.validate_remote().is_err());
  }
}

#[test]
fn diagnostics_do_not_expose_remote_text_or_transport_sources() {
  let remote = HandlerError::remote(1, "SECRET_MARKER").unwrap();
  let transport = HandlerError::Transport(std::io::Error::other("SECRET_MARKER").into());
  for error in [remote, transport, HandlerError::Internal] {
    assert!(!format!("{error:?}").contains("SECRET_MARKER"));
    assert!(!format!("{error}").contains("SECRET_MARKER"));
  }
}
