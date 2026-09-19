//! Current-user identity and an explicit owner-only, inheritable protected DACL.
use super::native::{checked, owned, wide, Local};
use anyhow::{Context, Result};
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::{
    Foundation::HANDLE,
    Security::{Authorization::*, *},
    System::Threading::*,
};

pub(super) struct User(Vec<usize>);
impl User {
    pub(super) fn of(process: HANDLE) -> Result<Self> {
        let mut token = std::ptr::null_mut();
        checked(unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) })?;
        let token = owned(token)?;
        let mut bytes = 0;
        unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                std::ptr::null_mut(),
                0,
                &mut bytes,
            );
        }
        anyhow::ensure!(bytes > 0, "missing process user token");
        let mut data = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
        checked(unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                data.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
            )
        })?;
        Ok(Self(data))
    }

    pub(super) fn current() -> Result<Self> {
        Self::of(unsafe { GetCurrentProcess() })
    }

    pub(super) fn sid(&self) -> PSID {
        unsafe { (*(self.0.as_ptr().cast::<TOKEN_USER>())).User.Sid }
    }

    pub(super) fn matches(&self, sid: PSID) -> bool {
        !sid.is_null() && unsafe { EqualSid(self.sid(), sid) } != 0
    }

    fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.sid().cast(), GetLengthSid(self.sid()) as usize) }
    }

    fn text(&self) -> Result<String> {
        let mut pointer = std::ptr::null_mut();
        checked(unsafe { ConvertSidToStringSidW(self.sid(), &mut pointer) })?;
        let _buffer = Local(pointer.cast());
        let mut length = 0;
        unsafe {
            while *pointer.add(length) != 0 {
                length += 1;
            }
        }
        String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, length) })
            .context("format Windows SID")
    }
}

pub(super) fn private_descriptor(user: &User) -> Result<Local> {
    let sid = user.text()?;
    descriptor(&format!("O:{sid}D:P(A;OICI;FA;;;{sid})"))
}

fn descriptor(text: &str) -> Result<Local> {
    let text = wide(text.as_ref())?;
    let mut pointer = std::ptr::null_mut();
    checked(unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut pointer,
            std::ptr::null_mut(),
        )
    })?;
    Ok(Local(pointer))
}

pub(super) fn validate(handle: HANDLE, user: &User) -> Result<()> {
    let mut owner = std::ptr::null_mut();
    let mut acl = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    let result = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut acl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    anyhow::ensure!(result == 0, "read private directory security: {result}");
    let descriptor = Local(descriptor);
    anyhow::ensure!(
        user.matches(owner),
        "private directory belongs to a different user"
    );
    let mut control = 0;
    let mut revision = 0;
    checked(unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) })?;
    anyhow::ensure!(
        control & SE_DACL_PROTECTED != 0,
        "private directory inherits permissions"
    );
    validate_acl(acl, user)
}

fn validate_acl(acl: *const ACL, user: &User) -> Result<()> {
    anyhow::ensure!(!acl.is_null(), "private directory has an unrestricted DACL");
    anyhow::ensure!(
        unsafe { (*acl).AceCount } == 1,
        "private directory must grant only its owner access"
    );
    let mut entry = std::ptr::null_mut();
    checked(unsafe { GetAce(acl, 0, &mut entry) })?;
    let header = unsafe { &*entry.cast::<ACE_HEADER>() };
    let length = header.AceSize as usize;
    let offset = (entry as usize)
        .checked_sub(acl as usize)
        .context("ACE outside ACL")?;
    anyhow::ensure!(
        offset >= std::mem::size_of::<ACL>()
            && offset + length <= unsafe { (*acl).AclSize as usize },
        "ACE outside ACL"
    );
    validate_ace(
        unsafe { std::slice::from_raw_parts(entry.cast::<u8>(), length) },
        user.bytes(),
    )
}

fn validate_ace(bytes: &[u8], owner: &[u8]) -> Result<()> {
    anyhow::ensure!(bytes.len() >= 8, "truncated ACE");
    anyhow::ensure!(bytes[0] == 0, "unexpected private directory ACE");
    anyhow::ensure!(
        u16::from_le_bytes([bytes[2], bytes[3]]) as usize == bytes.len(),
        "invalid ACE size"
    );
    let mask = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    anyhow::ensure!(
        mask == windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS,
        "private directory is not owner-writable"
    );
    anyhow::ensure!(
        bytes[1] & 3 == 3 && bytes[1] & INHERIT_ONLY_ACE as u8 == 0,
        "private directory children are not protected"
    );
    anyhow::ensure!(
        &bytes[8..] == owner,
        "private directory grants another identity access"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t493_private_descriptor_requests_protection_and_owner_only_inheritance() {
        let user = User::current().unwrap();
        let descriptor = private_descriptor(&user).unwrap();
        let mut control = 0;
        let mut revision = 0;
        checked(unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) })
            .unwrap();
        assert_ne!(control & SE_DACL_PROTECTED, 0);
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = std::ptr::null_mut();
        checked(unsafe {
            GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut acl, &mut defaulted)
        })
        .unwrap();
        assert_ne!(present, 0);
        validate_acl(acl, &user).unwrap();
        assert!(validate_acl(std::ptr::null(), &user).is_err());
        assert!(!user.matches(std::ptr::null_mut()));
        assert!(super::descriptor("invalid").is_err());
        assert!(super::descriptor("\0").is_err());
    }

    #[test]
    fn t493_private_acl_rejects_truncation_and_mutations() {
        let owner = [1, 0, 0, 0, 0, 0, 0, 5];
        let mut valid = vec![0, 3, 16, 0];
        valid.extend_from_slice(
            &windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS.to_le_bytes(),
        );
        valid.extend_from_slice(&owner);
        assert!(validate_ace(&valid, &owner).is_ok());
        for end in 0..valid.len() {
            assert!(validate_ace(&valid[..end], &owner).is_err());
        }
        for index in 2..valid.len() {
            for value in 0..=255u8 {
                if value == valid[index] {
                    continue;
                }
                let mut candidate = valid.clone();
                candidate[index] = value;
                assert!(
                    validate_ace(&candidate, &owner).is_err(),
                    "T493: accepted mutation at {index}"
                );
            }
        }
        for value in [1, 2, 5, 255] {
            let mut candidate = valid.clone();
            candidate[0] = value;
            assert!(validate_ace(&candidate, &owner).is_err());
        }
        for value in [0, 1, 2, 8, 11, 255] {
            let mut candidate = valid.clone();
            candidate[1] = value;
            assert!(validate_ace(&candidate, &owner).is_err());
        }
    }
}
