extern crate vk_api;
extern crate xml;

mod crawler;

use xml::EventReader;
use xml::attribute::OwnedAttribute;

use std::os::raw::c_char;
use std::ffi::CStr;
use std::ptr;
use std::fmt;

pub fn load_xml() {
    let vk_xml = EventReader::new(vk_api::VK_XML);
    // The vulkan registry
    let registry = crawler::crawl(vk_xml.into_iter(), VkRegistry::new(vk_api::VK_XML.len()));
}

pub struct VkRegistry {
    string_buffer: String,
    types: Vec<VkType>
}

impl<'a> VkRegistry {
    fn new(capacity: usize) -> VkRegistry {
        VkRegistry {
            string_buffer: String::with_capacity(capacity),
            types: Vec::with_capacity(256)
        }
    }

    fn push_type(&mut self, vk_type: VkType) -> Result<(), ()> {
        match vk_type {
            VkType::Unhandled => Err(()),
            vk_type           => {self.types.push(vk_type); Ok(())}
        }
    }

    /// Append a given attribute to the internal string buffer and return a pointer to a
    /// null-terminated string that represents the attribute
    fn append_str(&mut self, string: &str) -> *const c_char {
        // We want to have all of the string in one block of memory in order to save heap allocation time. 
        self.string_buffer.push_str(string);
        self.string_buffer.push('\0');

        (self.string_buffer.as_ptr() as usize + self.string_buffer.len() - string.len() - 1) as *const c_char
    }
}

enum VkMemberType {
    Var(*const c_char),
    Const(*const c_char),
    ConstPtr(*const c_char),
    MutPtr(*const c_char),
    ConstArray(*const c_char, usize),
    MutArray(*const c_char, usize),
    /// Default value to initialize with.
    Unknown
}

impl fmt::Debug for VkMemberType {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        use VkMemberType::*;
        if let Unknown = *self {
            write!(fmt, "Unknown")
        } else {
            let pt;
            let mut fmt_tuple = fmt.debug_tuple(
                match *self {
                    Var(p) => {pt = p; "Var"}
                    Const(p) => {pt = p; "Const"}
                    ConstPtr(p) => {pt = p; "ConstPtr"}
                    MutPtr(p) => {pt = p; "MutPtr"}
                    ConstArray(p, _) => {pt = p; "ConstArray"}
                    MutArray(p, _) => {pt = p; "MutArray"}
                    Unknown => unreachable!()
                });
            fmt_tuple.field(unsafe{ &if pt != ptr::null() {CStr::from_ptr(pt).to_str().unwrap()} else {"Error: Null Ptr"} });
            match *self {
                ConstArray(_, s) |
                MutArray(_, s) => fmt_tuple.field(&s),
                _ => &mut fmt_tuple
            }.finish()
        }
    }
}

struct VkMember {
    field_type: VkMemberType,
    field_name: *const c_char,
}

impl fmt::Debug for VkMember {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        fmt .debug_struct("VkMember")
            .field("type", &self.field_type)
            .field("name", unsafe{ &CStr::from_ptr(self.field_name) })
            .finish()
    }
}

impl VkMember {
    fn empty() -> VkMember {
        VkMember {
            field_type: VkMemberType::Unknown,
            field_name: ptr::null()
        }
    }

    fn type_ptr(&self) -> Option<*const c_char> {
        use VkMemberType::*;
        match self.field_type {
            Var(s) |
            Const(s) |
            ConstPtr(s) |
            MutPtr(s) |
            ConstArray(s, _) |
            MutArray(s, _) => Some(s),
            Unknown => None
        }
    }

    fn set_type(&mut self, field_type: *const c_char) {
        use VkMemberType::*;
        match self.field_type {
            Var(ref mut s) |
            Const(ref mut s) |
            ConstPtr(ref mut s) |
            MutPtr(ref mut s) | 
            ConstArray(ref mut s, _) |
            MutArray(ref mut s, _) => 
                if *s == ptr::null() {
                    *s = field_type
                } else {panic!("Field type already set")},
            Unknown => self.field_type = Var(field_type)

        }
    }

    fn change_type_var(&mut self) {
        use VkMemberType::*;
        match self.field_type {
            Unknown   => self.field_type = Var(ptr::null()),
            _ => panic!("Unexpected change type to var")
        }
    }

    fn change_type_const(&mut self) {
        use VkMemberType::*;
        match self.field_type {
            Var(s) |
            Const(s) => self.field_type = Const(s),
            ConstPtr(_) |
            MutPtr(_) => panic!("Attempted changing mutability of pointer"),
            ConstArray(_, _) |
            MutArray(_, _) => panic!("Attempted changing mutability of array"),
            Unknown   => self.field_type = Const(ptr::null())
        }
    }

    fn change_type_ptr(&mut self) {
        use VkMemberType::*;
        match self.field_type {
            Var(s) => self.field_type = MutPtr(s),
            Const(s) => self.field_type = ConstPtr(s),
            MutPtr(_) |
            ConstPtr(_) => panic!("Attempted to change type from pointer to pointer"),
            ConstArray(_, _) |
            MutArray(_, _) => panic!("Attempted to change type from array to pointer"),
            Unknown   => self.field_type = MutPtr(ptr::null())
        }
    }

    fn change_type_array(&mut self, size: usize) -> &mut VkMember {
        use VkMemberType::*;
        match self.field_type {
            Var(s) => self.field_type = MutArray(s, size),
            Const(s) => self.field_type = ConstArray(s, size),
            MutPtr(_) |
            ConstPtr(_) => panic!("Attempted to change type from pointer to array"),
            ConstArray(_, _) |
            MutArray(_, _) => panic!("Attempted to change type from array to array"),
            Unknown => panic!("Attempted to change type to array without type identifier")
        }
        self
    }

    fn set_name(&mut self, field_name: *const c_char) {
        if self.field_name != ptr::null() {
            panic!("Unexpected \"name\" tag");
        } else {
            self.field_name = field_name;
        }
    }
}

/// A variant of a vulkan enum
#[derive(Debug)]
struct VkVariant {
    name: *const c_char,
    value: isize
}

#[derive(Debug)]
enum VkType {
    Struct {
        name: *const c_char,
        fields: Vec<VkMember>
    },

    Union {
        name: *const c_char,
        variants: Vec<VkMember>
    },

    Enum {
        name: *const c_char,
        variants: Vec<VkVariant>
    },

    BaseType {
        /// The type that is being aliased
        ty: *const c_char,
        /// The name of the new type
        name: *const c_char
    },

    Unhandled
}