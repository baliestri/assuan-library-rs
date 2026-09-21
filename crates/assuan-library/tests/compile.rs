//! Compilation contracts for consumers of the public facade.

#[test]
fn public_api_compile_contracts() {
  let tests = trybuild::TestCases::new();
  tests.pass("tests/ui/attribute_ok.rs");
  tests.compile_fail("tests/ui/attribute_bad_signature.rs");
  tests.compile_fail("tests/ui/command_newline.rs");
  tests.compile_fail("tests/ui/borrowed_event_escape.rs");
  tests.compile_fail("tests/ui/second_transaction.rs");
  tests.compile_fail("tests/ui/secret_clone.rs");
  tests.compile_fail("tests/ui/attribute_bad_return.rs");
  tests.compile_fail("tests/ui/attribute_reserved.rs");
  tests.compile_fail("tests/ui/attribute_description.rs");
  tests.compile_fail("tests/ui/attribute_borrow_escape.rs");
  tests.compile_fail("tests/ui/attribute_not_send.rs");
  tests.compile_fail("tests/ui/sexpr_dynamic.rs");
}
