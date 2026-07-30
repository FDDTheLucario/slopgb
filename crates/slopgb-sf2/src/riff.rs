//! Minimal hand-rolled RIFF chunk framing shared by [`crate::reader`] and
//! [`crate::writer`]: a RIFF file is a sequence of `FourCC + u32-LE length +
//! data` chunks, each padded to an even byte length (the pad byte is not
//! counted in `length`). A `RIFF`/`LIST` chunk's data begins with a 4-byte
//! form-type FourCC followed by nested sub-chunks.

/// One parsed top-level chunk: its 4-byte id and its body (excluding the
/// id/length header and any trailing pad byte).
pub struct RiffChunk<'a> {
    pub id: [u8; 4],
    pub data: &'a [u8],
}

/// Split `buf` into a sequence of sibling chunks (does not recurse into
/// `LIST`/`RIFF` bodies — call [`parse_chunks`] again on `data` for that).
pub fn parse_chunks(buf: &[u8]) -> Result<Vec<RiffChunk<'_>>, String> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < buf.len() {
        if pos + 8 > buf.len() {
            return Err(format!("RIFF chunk header truncated at offset {pos}"));
        }
        let id = [buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]];
        let len = read_u32_le(&buf[pos + 4..pos + 8]) as usize;
        let data_start = pos + 8;
        let data_end = data_start
            .checked_add(len)
            .filter(|&e| e <= buf.len())
            .ok_or_else(|| format!("RIFF chunk {id:?} length {len} overruns buffer"))?;
        out.push(RiffChunk {
            id,
            data: &buf[data_start..data_end],
        });
        pos = data_end + (len % 2); // even-align, skipping the pad byte
    }
    Ok(out)
}

/// Split a `LIST`/`RIFF` chunk's data into its form-type FourCC and the
/// remaining (sub-chunk) bytes.
pub fn form_and_body(data: &[u8]) -> Result<([u8; 4], &[u8]), String> {
    if data.len() < 4 {
        return Err("LIST/RIFF chunk too short for a form type".to_string());
    }
    let form = [data[0], data[1], data[2], data[3]];
    Ok((form, &data[4..]))
}

/// Build one RIFF chunk: id + LE length + body (+ a zero pad byte if `body`
/// is odd-length).
pub fn write_chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + body.len() + 1);
    out.extend_from_slice(id);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

/// Build a `LIST` chunk with the given form-type FourCC wrapping `body`
/// (the concatenated bytes of its sub-chunks).
pub fn write_list(form: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut inner = Vec::with_capacity(4 + body.len());
    inner.extend_from_slice(form);
    inner.extend_from_slice(body);
    write_chunk(b"LIST", &inner)
}

pub fn read_u16_le(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

pub fn read_u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

pub fn read_i16_le(b: &[u8]) -> i16 {
    i16::from_le_bytes([b[0], b[1]])
}

/// Read a NUL-padded fixed/variable-length ASCII string chunk (INFO
/// sub-chunks, `phdr`/`inst`/`shdr` name fields), stopping at the first NUL.
pub fn read_cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// Write a name into a fixed-size NUL-padded field (SF2 names are 20 bytes in
/// `phdr`/`inst`/`shdr`), truncating if too long.
pub fn write_fixed_str(out: &mut Vec<u8>, s: &str, width: usize) {
    let bytes = s.as_bytes();
    let n = bytes.len().min(width);
    out.extend_from_slice(&bytes[..n]);
    out.resize(out.len() + (width - n), 0);
}

#[cfg(test)]
#[path = "riff_tests.rs"]
mod tests;
