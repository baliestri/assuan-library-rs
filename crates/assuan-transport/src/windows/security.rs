use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use widestring::U16CString;
use winsafe::{HACCESSTOKEN, HPROCESS, TokenInfo, co};

use crate::TransportError;

pub(super) fn current_user_sid() -> Result<String, TransportError> {
  let token = HPROCESS::GetCurrentProcess()
    .OpenProcessToken(co::TOKEN::QUERY)
    .map_err(|_| return TransportError::AccessPolicy)?;
  return token_user_sid(&token);
}

fn token_user_sid(token: &HACCESSTOKEN) -> Result<String, TransportError> {
  let information = token
    .GetTokenInformation(co::TOKEN_INFORMATION_CLASS::User)
    .map_err(|_| return TransportError::AccessPolicy)?;
  let TokenInfo::User(user) = information else {
    return Err(TransportError::AccessPolicy);
  };
  let sid = user.User.Sid().ok_or(TransportError::AccessPolicy)?;
  return winsafe::ConvertSidToStringSid(sid).map_err(|_| return TransportError::AccessPolicy);
}

pub(super) fn current_user_descriptor() -> Result<SecurityDescriptor, TransportError> {
  return descriptor_for_sid(&current_user_sid()?);
}

fn descriptor_for_sid(sid: &str) -> Result<SecurityDescriptor, TransportError> {
  if !sid.starts_with("S-1-")
    || !sid[4..].bytes().all(|byte| return byte.is_ascii_digit() || byte == b'-')
  {
    return Err(TransportError::AccessPolicy);
  }
  let sddl = U16CString::from_str(format!("O:{sid}D:P(A;;GA;;;{sid})"))
    .map_err(|_| return TransportError::AccessPolicy)?;
  return SecurityDescriptor::deserialize(&sddl).map_err(|_| return TransportError::AccessPolicy);
}

#[cfg(test)]
mod tests {
  use interprocess::os::windows::security_descriptor::AsSecurityDescriptorExt;

  use super::*;

  #[test]
  fn acl_contains_only_the_current_user_and_is_protected() {
    let sid = current_user_sid().unwrap();
    let descriptor = current_user_descriptor().unwrap();
    // OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION.
    let serialized = descriptor
      .serialize(0x0000_0001 | 0x0000_0004, |value| return value.to_string_lossy())
      .unwrap();
    // Windows may serialize numeric SIDs using SDDL aliases (e.g. LA on CI).
    // Normalize only the expected owner; still require exactly one protected
    // allow ACE with full access for that same identity.
    let owner = SecurityDescriptor::deserialize(&U16CString::from_str(format!("O:{sid}")).unwrap())
      .unwrap()
      .serialize(0x0000_0001, |value| return value.to_string_lossy())
      .unwrap();
    let trustee = owner.strip_prefix("O:").unwrap();
    assert_eq!(serialized, format!("{owner}D:P(A;;GA;;;{trustee})"));
  }

  #[test]
  fn well_known_sid_serialization_preserves_the_exact_access_policy() {
    let descriptor = descriptor_for_sid("S-1-5-18").unwrap();
    let serialized = descriptor
      .serialize(0x0000_0001 | 0x0000_0004, |value| return value.to_string_lossy())
      .unwrap();
    assert_eq!(serialized, "O:SYD:P(A;;GA;;;SY)");
  }

  #[test]
  fn invalid_sid_never_falls_back_to_a_default_descriptor() {
    for sid in ["", "invalid", "S-1-", "S-1-5-21)(A;;GA;;;WD)", "S-1-5-21\0"] {
      assert!(matches!(descriptor_for_sid(sid), Err(TransportError::AccessPolicy)));
    }
  }
}
