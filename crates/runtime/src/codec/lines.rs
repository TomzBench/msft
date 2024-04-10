//! lines

use super::Decode;
use bytes::BytesMut;
use std::str;

/// A simple [`Decoder`] and [`Encoder`] implementation that splits up data into lines. This uses
/// the `\n` character as the line ending on all platforms.
///
/// NOTE this implementation (much like much of this module) is lifted heavily from
/// tokio_util::codec. However, unlike LinesCodec, we do not need to worry about max length because
/// the Stream types will handle overflows
pub struct LinesDecoder {
    // Current index into the buffer so we avoid re-scanning the buffer each call to decode
    index: usize,
}

impl Default for LinesDecoder {
    fn default() -> Self {
        Self { index: 0 }
    }
}

impl Decode for LinesDecoder {
    type Item = String;
    type Error = str::Utf8Error;
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(offset) = src[self.index..src.len()].iter().position(|b| *b == b'\n') {
            let line = src.split_to(self.index + offset + 1);
            if line.len() > 2 {
                str::from_utf8(&line[..line.len() - 2]).map(|s| Some(s.to_string()))
            } else {
                Ok(None)
            }
        } else {
            self.index = src.len();
            Ok(None)
        }
    }
}

/*
pub struct LinesEncoder {}

impl<T> Encoder<T> for LinesEncoder
where
    T: AsRef<str>,
{
    type Error = StreamError;
    fn encode(&mut self, item: T, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let line = item.as_ref();
        dst.reserve(line.len() + 1);
        dst.put(line.as_bytes());
        dst.put_u8(b'\n');
        Ok(())
    }
}
*/
