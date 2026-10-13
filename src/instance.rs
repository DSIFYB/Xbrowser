use crate::wide;
use windows_sys::Win32::{Foundation::*, System::Threading::CreateMutexW};
pub struct Instance(HANDLE);
impl Instance {
    pub fn acquire(name: &str) -> std::io::Result<Option<Self>> {
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 0, wide(name).as_ptr());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(handle);
                Ok(None)
            } else {
                Ok(Some(Self(handle)))
            }
        }
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_instance_blocks_second_writer() {
        let name = format!("Local\\AxiomInstanceTest{}", std::process::id());
        let first = Instance::acquire(&name).unwrap().unwrap();
        assert!(Instance::acquire(&name).unwrap().is_none());
        drop(first);
        assert!(Instance::acquire(&name).unwrap().is_some());
    }
}
