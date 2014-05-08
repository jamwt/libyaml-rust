use libc;

use ffi;
use event::{YamlEvent};
use document::{YamlDocument};
use codecs;

use std::cast;
use std::io;
use std::c_vec::CVec;

#[deriving(Eq)]
#[deriving(Show)]
pub enum YamlErrorType {
    YamlNoError,
    YamlMemoryError,
    YamlReaderError,
    YamlScannerError,
    YamlParserError,
    YamlComposerError,
    YamlWriterError,
    YamlEmitterError,
}

#[deriving(Eq)]
#[deriving(Show)]
pub struct YamlMark {
    index: uint,
    line: uint,
    column: uint
}

impl YamlMark {
    pub fn conv(mark: &ffi::yaml_mark_t) -> YamlMark {
        YamlMark {
            index: mark.index as uint,
            line: mark.line as uint,
            column: mark.column as uint
        }
    }
}

#[deriving(Eq)]
#[deriving(Show)]
pub struct YamlError {
    kind: YamlErrorType,
    problem: Option<~str>,
    byte_offset: uint,
    problem_mark: YamlMark,
    context: Option<~str>,
    context_mark: YamlMark,
}

pub struct YamlEventStream<P> {
    parser: ~P,
}

impl<P:YamlParser> YamlEventStream<P> {
    pub fn next_event(&mut self) -> Result<YamlEvent, YamlError> {
        unsafe {
            match self.parser.parse_event() {
                Some(evt) => Ok(evt),
                None => Err(self.parser.base_parser_ref().get_error())
            }
        }
    }
}

pub struct YamlDocumentStream<P> {
    parser: ~P,
}

impl<P:YamlParser> YamlDocumentStream<P> {
    pub fn next_document(&mut self) -> Result<~YamlDocument, YamlError> {
        unsafe {
            match YamlDocument::parser_load(&mut self.parser.base_parser_ref().parser_mem) {
                Some(doc) => Ok(doc),
                None => Err(self.parser.base_parser_ref().get_error())
            }
        }
    }
}

pub struct InternalEvent {
    event_mem: ffi::yaml_event_t
}

impl Drop for InternalEvent {
    fn drop(&mut self) {
        unsafe {
            self.event_mem.delete()
        }
    }
}

pub trait YamlParser {
    unsafe fn base_parser_ref<'r>(&'r mut self) -> &'r mut YamlBaseParser;

    unsafe fn parse_event(&mut self) -> Option<YamlEvent> {
        let mut event = InternalEvent {
            event_mem: ffi::yaml_event_t::new()
        };

        if !self.base_parser_ref().parse(&mut event.event_mem) {
            None
        } else {
            Some(YamlEvent::load(&event.event_mem))
        }
    }

    fn parse(~self) -> YamlEventStream<Self> {
        YamlEventStream {
            parser: self,
        }
    }

    fn load(~self) -> YamlDocumentStream<Self> {
        YamlDocumentStream {
            parser: self,
        }
    }
}

extern fn handle_reader_cb(data: *mut YamlIoParser, buffer: *mut u8, size: libc::size_t, size_read: *mut libc::size_t) -> libc::c_int {
    unsafe {
        let mut buf = CVec::new(buffer, size as uint);
        let parser = &mut *data;
        match parser.reader.read(buf.as_mut_slice()) {
            Ok(size) => {
                *size_read = size as libc::size_t;
                return 1;
            },
            Err(err) => {
                match err.kind {
                    io::EndOfFile => {
                        *size_read = 0;
                        return 1;
                    },
                    _ => {
                        return 0;
                    }
                }
            }
        }
    }
}

pub struct YamlBaseParser {
    parser_mem: ffi::yaml_parser_t,
}

impl YamlBaseParser {
    fn new() -> YamlBaseParser {
        YamlBaseParser {
            parser_mem: ffi::yaml_parser_t::new()
        }
    }

    unsafe fn initialize(&mut self) -> bool {
        ffi::yaml_parser_initialize(&mut self.parser_mem) != 0
    }

    unsafe fn set_input_string(&mut self, input: *u8, size: uint) {
        ffi::yaml_parser_set_input_string(&mut self.parser_mem, input, size as libc::size_t);
    }

    unsafe fn parse(&mut self, event: &mut ffi::yaml_event_t) -> bool {
        ffi::yaml_parser_parse(&mut self.parser_mem, event) != 0
    }

    unsafe fn get_error(&self) -> YamlError {
        let kind = match self.parser_mem.error {
            ffi::YAML_NO_ERROR => YamlNoError,
            ffi::YAML_READER_ERROR => YamlReaderError,
            ffi::YAML_SCANNER_ERROR => YamlScannerError,
            ffi::YAML_PARSER_ERROR => YamlParserError,
            ffi::YAML_COMPOSER_ERROR => YamlComposerError,
            ffi::YAML_WRITER_ERROR => YamlWriterError,
            ffi::YAML_EMITTER_ERROR => YamlEmitterError,
            _ => fail!("unknown error type")
        };

        YamlError {
            kind: kind,
            problem: codecs::decode_c_str(self.parser_mem.problem as *ffi::yaml_char_t),
            byte_offset: self.parser_mem.problem_offset as uint,
            problem_mark: YamlMark::conv(&self.parser_mem.problem_mark),
            context: codecs::decode_c_str(self.parser_mem.context as *ffi::yaml_char_t),
            context_mark: YamlMark::conv(&self.parser_mem.context_mark),
        }
    }
}

impl Drop for YamlBaseParser {
    fn drop(&mut self) {
        unsafe {
            ffi::yaml_parser_delete(&mut self.parser_mem);
        }
    }
}

pub struct YamlByteParser<'r> {
    base_parser: YamlBaseParser
}

impl<'r> YamlParser for YamlByteParser<'r> {
    unsafe fn base_parser_ref<'r>(&'r mut self) -> &'r mut YamlBaseParser {
        &mut self.base_parser
    }
}

impl<'r> YamlByteParser<'r> {
    pub fn init(bytes: &'r [u8]) -> ~YamlByteParser<'r> {
        let mut parser = ~YamlByteParser {
            base_parser: YamlBaseParser::new()
        };

        unsafe {
            if !parser.base_parser.initialize() {
                fail!("failed to initialize yaml_parser_t");
            }
            parser.base_parser.set_input_string(bytes.as_ptr(), bytes.len());
        }

        parser
    }
}

pub struct YamlIoParser {
    base_parser: YamlBaseParser,
    reader: ~Reader,
}

impl<'r> YamlParser for YamlIoParser {
    unsafe fn base_parser_ref<'r>(&'r mut self) -> &'r mut YamlBaseParser {
        &mut self.base_parser
    }
}

impl YamlIoParser {
    pub fn init(reader: ~Reader) -> ~YamlIoParser {
        let mut parser = ~YamlIoParser {
            base_parser: YamlBaseParser::new(),
            reader: reader
        };

        unsafe {
            if !parser.base_parser.initialize() {
                fail!("failed to initialize yaml_parser_t");
            }

            ffi::yaml_parser_set_input(&mut parser.base_parser.parser_mem, handle_reader_cb, cast::transmute(&mut *parser));
        }

        parser
    }
} 

#[cfg(test)]
mod test {
    use event::*;
    use document;
    use parser;
    use parser::YamlParser;
    use ffi;
    use std::io;

    #[test]
    fn test_byte_parser() {
        let data = "[1, 2, 3]";
        let parser = parser::YamlByteParser::init(data.as_bytes());
        let expected = vec![
            YamlStreamStartEvent(ffi::YamlUtf8Encoding),
            YamlDocumentStartEvent(None, ~[], true),
            YamlSequenceStartEvent(YamlSequenceParam{anchor: None, tag: None, implicit: true, style: ffi::YamlFlowSequenceStyle}),
            YamlScalarEvent(YamlScalarParam{anchor: None, tag: None, value: "1".to_owned(), plain_implicit: true, quoted_implicit: false, style: ffi::YamlPlainScalarStyle}),
            YamlScalarEvent(YamlScalarParam{anchor: None, tag: None, value: "2".to_owned(), plain_implicit: true, quoted_implicit: false, style: ffi::YamlPlainScalarStyle}),
            YamlScalarEvent(YamlScalarParam{anchor: None, tag: None, value: "3".to_owned(), plain_implicit: true, quoted_implicit: false, style: ffi::YamlPlainScalarStyle}),
            YamlSequenceEndEvent,
            YamlDocumentEndEvent(true),
            YamlStreamEndEvent
        ];

        let mut produced = Vec::new();
        let mut stream = parser.parse();

        loop {
            match stream.next_event() {
                Ok(YamlNoEvent) => {
                    break;
                },
                Ok(evt) => {
                    produced.push(evt);
                },
                Err(err) => {
                    fail!("{:?}", err);
                }
            }
        }

        assert_eq!(expected, produced);
    }

    #[test]
    fn test_io_parser() {
        let data = "[1, 2, 3]";
        let reader = ~io::BufReader::new(data.as_bytes());
        let parser = parser::YamlIoParser::init(reader);
        let expected = vec![
            YamlStreamStartEvent(ffi::YamlUtf8Encoding),
            YamlDocumentStartEvent(None, ~[], true),
            YamlSequenceStartEvent(YamlSequenceParam{anchor: None, tag: None, implicit: true, style: ffi::YamlFlowSequenceStyle}),
            YamlScalarEvent(YamlScalarParam{anchor: None, tag: None, value: "1".to_owned(), plain_implicit: true, quoted_implicit: false, style: ffi::YamlPlainScalarStyle}),
            YamlScalarEvent(YamlScalarParam{anchor: None, tag: None, value: "2".to_owned(), plain_implicit: true, quoted_implicit: false, style: ffi::YamlPlainScalarStyle}),
            YamlScalarEvent(YamlScalarParam{anchor: None, tag: None, value: "3".to_owned(), plain_implicit: true, quoted_implicit: false, style: ffi::YamlPlainScalarStyle}),
            YamlSequenceEndEvent,
            YamlDocumentEndEvent(true),
            YamlStreamEndEvent
        ];

        let mut produced = Vec::new();
        let mut stream = parser.parse();

        loop {
            match stream.next_event() {
                Ok(YamlNoEvent) => {
                    break;
                },
                Ok(evt) => {
                    produced.push(evt);
                },
                Err(err) => {
                    fail!("{:?}", err);
                }
            }
        }

        assert_eq!(expected, produced);
    }

    #[test]
    fn test_byte_parser_mapping() {
        let data = "{\"a\": 1, \"b\":2}";
        let parser = parser::YamlByteParser::init(data.as_bytes());
        let expected = vec![
            YamlStreamStartEvent(ffi::YamlUtf8Encoding),
            YamlDocumentStartEvent(None, ~[], true),
            YamlMappingStartEvent(YamlSequenceParam{anchor: None, tag: None, implicit: true, style: ffi::YamlFlowSequenceStyle}),
            YamlScalarEvent(YamlScalarParam{anchor: None, tag: None, value: "a".to_owned(), plain_implicit: false, quoted_implicit: true, style: ffi::YamlDoubleQuotedScalarStyle}),
            YamlScalarEvent(YamlScalarParam{anchor: None, tag: None, value: "1".to_owned(), plain_implicit: true, quoted_implicit: false, style: ffi::YamlPlainScalarStyle}),
            YamlScalarEvent(YamlScalarParam{anchor: None, tag: None, value: "b".to_owned(), plain_implicit: false, quoted_implicit: true, style: ffi::YamlDoubleQuotedScalarStyle}),
            YamlScalarEvent(YamlScalarParam{anchor: None, tag: None, value: "2".to_owned(), plain_implicit: true, quoted_implicit: false, style: ffi::YamlPlainScalarStyle}),
            YamlMappingEndEvent,
            YamlDocumentEndEvent(true),
            YamlStreamEndEvent
        ];

        let mut produced = Vec::new();
        let mut stream = parser.parse();

        loop {
            match stream.next_event() {
                Ok(YamlNoEvent) => {
                    break;
                },
                Ok(evt) => {
                    produced.push(evt);
                },
                Err(err) => {
                    fail!("{:?}", err);
                }
            }
        }

        assert_eq!(expected, produced);
    }

    #[test]
    fn test_parser_error() {
        let data = "\"ab";
        let parser = parser::YamlByteParser::init(data.as_bytes());
        let mut stream = parser.parse();

        let stream_start = stream.next_event();
        assert_eq!(Ok(YamlStreamStartEvent(ffi::YamlUtf8Encoding)), stream_start);

        let stream_err = stream.next_event();
        match stream_err {
            Ok(evt) => fail!("unexpected result: {:?}", evt),
            Err(err) => assert_eq!(parser::YamlScannerError, err.kind)
        }
    }

    #[test]
    fn test_document() {
        let data = "[1, 2, 3]";
        let parser = parser::YamlByteParser::init(data.as_bytes());
        let mut stream = parser.load();

        match stream.next_document() {
            Err(e) => fail!("unexpected result: {:?}", e),
            Ok(doc) => match doc.root() {
                document::YamlSequenceNode(seq) => {
                    let values = seq.values().map(|node| {
                        match node {
                            document::YamlScalarNode(scalar) => scalar.get_value(),
                            _ => fail!("unexpected scalar: {:?}", node)
                        }
                    }).collect();
                    assert_eq!(~["1".to_owned(), "2".to_owned(), "3".to_owned()], values)
                },
                _ => fail!("unexpected result: {:?}", doc)
            }
        }
    }

    #[test]
    fn test_mapping_document() {
        let data = "{\"a\": 1, \"b\": 2}";
        let parser = parser::YamlByteParser::init(data.as_bytes());
        let mut stream = parser.load();

        match stream.next_document() {
            Err(e) => fail!("unexpected result: {:?}", e),
            Ok(doc) => match doc.root() {
                document::YamlMappingNode(seq) => {
                    let values = seq.pairs().map(|(key, value)| {
                        (
                            match key {
                                document::YamlScalarNode(scalar) => scalar.get_value(),
                                _ => fail!("unexpected scalar: {:?}", key)
                            },
                            match value {
                                document::YamlScalarNode(scalar) => scalar.get_value(),
                                _ => fail!("unexpected scalar: {:?}", value)
                            }
                        )
                    }).collect();
                    assert_eq!(~[("a".to_owned(), "1".to_owned()), ("b".to_owned(), "2".to_owned())], values)
                },
                _ => fail!("unexpected result: {:?}", doc)
            }
        }
    }
}
