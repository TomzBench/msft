//! codec

pub mod lines;

/// I/O completions will try and decode the incoming bytes and yeild some Items
pub trait Decode {
    type Item;
    type Error: std::error::Error;

    /// Decode the bytes
    ///
    /// When bytes are ready, 1 of 3 things may be the case.
    ///
    /// 1. The buffer contains less than a full frame.
    /// 2. The buffer contains exactly a full frame.
    /// 3. The buffer contains more than a full frame.
    ///
    /// In 1st situation, the decoder should return Ok(None).
    ///
    /// In the 2nd situation the decoder can remove all bytes from the buffer and return
    /// Ok(decoded_frame)
    ///
    /// In the 3rd situation, the decoder should remove the frame from the buffer with methods such
    /// as [`bytes::BytesMut::split_to`] or [`bytes::Buf::advance`], and return Ok(Some(decoded_frame)).
    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error>;
}

/// Something that can be encoded into an array of bytes
pub trait SinkEncode {
    type Error: std::error::Error;
    fn sink_encode(&self, dst: &mut bytes::BytesMut) -> Result<(), Self::Error>;
}

/// Something that knows how many bytes are needed to encode itself
pub trait SinkEncodeLen {
    fn sink_encode_len(&self) -> usize;
}
