use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

pub const ABI_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolResult {
    pub content: Vec<ContentItem>,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ContentItem {
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
}

impl ToolResult {
    #[must_use]
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            content: vec![ContentItem {
                type_: "text".into(),
                text: text.into(),
            }],
            is_error: false,
        }
    }

    #[must_use]
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![ContentItem {
                type_: "text".into(),
                text: text.into(),
            }],
            is_error: true,
        }
    }
}

/// # Safety
///
/// `ptr` must be a valid, null-terminated C string allocated by the plugin.
/// The caller must not use the returned `String` after the plugin is unloaded.
pub unsafe fn c_str_to_string(ptr: *const c_char) -> String {
    CStr::from_ptr(ptr).to_string_lossy().into_owned()
}

pub fn string_to_c_str(s: String) -> *mut c_char {
    CString::new(s).unwrap_or_default().into_raw()
}

/// # Safety
///
/// `ptr` must have been allocated by a plugin via `string_to_c_str`.
/// Calling with a null pointer is safe and treated as a no-op.
pub unsafe fn free_c_str(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}
