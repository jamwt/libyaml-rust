use libc;

use codecs;
use ffi;
use ffi::yaml_node_type_t::*;
use error::YamlMark;
use event::{YamlVersionDirective, YamlTagDirective};

use std::ptr;
use std::mem;
use std::c_str::{CString, ToCStr};

pub struct YamlDocument {
    document_mem: ffi::yaml_document_t
}

impl YamlDocument {
    pub unsafe fn parser_load(parser: &mut ffi::yaml_parser_t) -> Option<Box<YamlDocument>> {
        let mut document = box YamlDocument {
            document_mem: mem::uninitialized()
        };

        if ffi::yaml_parser_load(parser, &mut document.document_mem) == 0 {
            None
        } else {
            Some(document)
        }
    }

    pub fn is_empty(&self) -> bool {
        unsafe {
            ffi::yaml_document_get_root_node(&self.document_mem) == ptr::null()
        }
    }

    pub fn init(version_directive: Option<YamlVersionDirective>,
        tag_directives: &[YamlTagDirective],
        start_implicit: bool, end_implicit: bool) -> Box<YamlDocument>
    {
        let mut vsn_dir = ffi::yaml_version_directive_t { major: 0, minor: 0 };
        let c_vsn_dir = match version_directive {
            None => ptr::null(),
            Some(vsn) => {
                vsn_dir.major = vsn.major as libc::c_int;
                vsn_dir.minor = vsn.minor as libc::c_int;
                &vsn_dir as *const ffi::yaml_version_directive_t
            }
        };

        let c_strs: Vec<(CString, CString)> = tag_directives.iter().map(|tag| {
            (tag.handle.to_c_str(), tag.prefix.to_c_str())
        }).collect();
        let c_tag_dirs: Vec<ffi::yaml_tag_directive_t> = c_strs.iter().map(|tuple| {
            ffi::yaml_tag_directive_t {
                handle: tuple.0.as_ptr(),
                prefix: tuple.1.as_ptr(),
            }
        }).collect();
        let tag_dir_start = c_tag_dirs.as_ptr();
        let c_start_implicit = if start_implicit { 1 } else { 0 };
        let c_end_implicit = if end_implicit { 1 } else { 0 };
        unsafe {
            let mut document = box YamlDocument {
                document_mem: mem::uninitialized()
            };

            let tag_dir_end = tag_dir_start.offset(c_tag_dirs.len() as int);
            if ffi::yaml_document_initialize(&mut document.document_mem, c_vsn_dir,
                tag_dir_start, tag_dir_end, c_start_implicit, c_end_implicit) == 0
            {
                panic!("yaml_document_initialize failed!")
            }

            document
        }
    }

    unsafe fn load<'r>(&'r self, node_ptr: *const ffi::yaml_node_t) -> YamlNode<'r> {
        if node_ptr == ptr::null() {
            panic!("empty node")
        }
        let node = &*node_ptr;
        match node.node_type {
            YAML_SCALAR_NODE => {
                let scalar_data: &ffi::yaml_scalar_node_t = mem::transmute(&node.data);
                YamlNode::YamlScalarNode(YamlScalarData {
                    node: node,
                    data: scalar_data
                })
            },
            YAML_SEQUENCE_NODE => {
                let sequence_data: &ffi::yaml_sequence_node_t = mem::transmute(&node.data);
                YamlNode::YamlSequenceNode(YamlSequenceData {
                    doc: self,
                    node: node,
                    data: sequence_data
                })
            },
            YAML_MAPPING_NODE => {
                let mapping_data: &ffi::yaml_sequence_node_t = mem::transmute(&node.data);
                YamlNode::YamlMappingNode(YamlMappingData {
                    doc: self,
                    node: node,
                    data: mapping_data
                })
            },
            _ => panic!("invalid node")
        }
    }

    unsafe fn get_node<'r>(&'r self, index: libc::c_int) -> YamlNode<'r> {
        let node_ptr = ffi::yaml_document_get_node(&self.document_mem, index);
        self.load(node_ptr)
    }

    pub fn root<'r>(&'r self) -> Option<YamlNode<'r>> {
        unsafe {
            let node_ptr = ffi::yaml_document_get_root_node(&self.document_mem);
            if node_ptr == ptr::null() {
                None
            } else {
                Some(self.load(node_ptr))
            }
        }
    }
}

impl Drop for YamlDocument {
    fn drop(&mut self) {
        unsafe {
            ffi::yaml_document_delete(&mut self.document_mem);
        }
    }
}

pub enum YamlNode<'r> {
    YamlScalarNode(YamlScalarData<'r>),
    YamlSequenceNode(YamlSequenceData<'r>),
    YamlMappingNode(YamlMappingData<'r>),
}

pub trait YamlNodeData {
    unsafe fn internal_node<'r>(&'r self) -> &'r ffi::yaml_node_t;

    fn tag(&self) -> Option<String> {
        unsafe {
            codecs::decode_c_str(self.internal_node().tag)
        }
    }

    fn start_mark(&self) -> YamlMark {
        unsafe {
            YamlMark::conv(&self.internal_node().start_mark)
        }
    }

    fn end_mark(&self) -> YamlMark {
        unsafe {
            YamlMark::conv(&self.internal_node().end_mark)
        }
    }
}

pub struct YamlScalarData<'r> {
    node: &'r ffi::yaml_node_t,
    data: &'r ffi::yaml_scalar_node_t
}

impl<'r> YamlNodeData for YamlScalarData<'r> {
    unsafe fn internal_node<'a>(&'a self) -> &'a ffi::yaml_node_t {
        self.node
    }
}

impl<'r> YamlScalarData<'r> {
    pub fn get_value(&self) -> String {
        codecs::decode_buf(self.data.value, self.data.length).unwrap()
    }

    pub fn style(&self) -> ffi::YamlScalarStyle {
        self.data.style
    }
}

pub struct YamlSequenceData<'r> {
    doc: &'r YamlDocument,
    node: &'r ffi::yaml_node_t,
    data: &'r ffi::yaml_sequence_node_t
}

impl<'r> YamlNodeData for YamlSequenceData<'r> {
    unsafe fn internal_node<'a>(&'a self) -> &'a ffi::yaml_node_t {
        self.node
    }
}

impl<'r> YamlSequenceData<'r> {
    pub fn values(&self) -> YamlSequenceIter<'r> {
        YamlSequenceIter {
            doc: self.doc,
            top: self.data.items.top as *const libc::c_int,
            ptr: self.data.items.start as *const libc::c_int
        }
    }
}

pub struct YamlSequenceIter<'r> {
    doc: &'r YamlDocument,
    top: *const libc::c_int,
    ptr: *const libc::c_int
}

impl<'r> Iterator for YamlSequenceIter<'r> {
    type Item = YamlNode<'r>;

    fn next(&mut self) -> Option<YamlNode<'r>> {
        if self.ptr == self.top {
            None
        } else {
            unsafe {
                let next_node = self.doc.get_node(*self.ptr);

                self.ptr = self.ptr.offset(1);

                Some(next_node)
            }
        }
    }
}

pub struct YamlMappingData<'r> {
    doc: &'r YamlDocument,
    node: &'r ffi::yaml_node_t,
    data: &'r ffi::yaml_sequence_node_t
}

impl<'r> YamlNodeData for YamlMappingData<'r> {
    unsafe fn internal_node<'a>(&'a self) -> &'a ffi::yaml_node_t {
        self.node
    }
}

impl<'r> YamlMappingData<'r> {
    pub fn pairs(&self) -> YamlMappingIter<'r> {
        YamlMappingIter {
            doc: self.doc,
            top: self.data.items.top as *const ffi::yaml_node_pair_t,
            ptr: self.data.items.start as *const ffi::yaml_node_pair_t
        }
    }
}

pub struct YamlMappingIter<'r> {
    doc: &'r YamlDocument,
    top: *const ffi::yaml_node_pair_t,
    ptr: *const ffi::yaml_node_pair_t
}

impl<'r> Iterator for YamlMappingIter<'r> {
    type Item = (YamlNode<'r>, YamlNode<'r>);

    fn next(&mut self) -> Option<(YamlNode<'r>, YamlNode<'r>)> {
        if self.ptr == self.top {
            None
        } else {
            unsafe {
                let next_key = self.doc.get_node((*self.ptr).key);
                let next_value = self.doc.get_node((*self.ptr).value);

                self.ptr = self.ptr.offset(1);

                Some((next_key, next_value))
            }
        }
    }
}

