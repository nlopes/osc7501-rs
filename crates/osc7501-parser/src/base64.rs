use crate::Error;

const INVALID: u8 = 255;
const fn table() -> [u8; 256] {
    let mut values = [INVALID; 256];
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut i: u8 = 0;
    while i < 64 {
        values[alphabet[i as usize] as usize] = i;
        i += 1;
    }
    values
}
const DECODE: [u8; 256] = table();

pub(crate) fn decode(input: &[u8], output: &mut [u8]) -> Result<usize, Error> {
    let padding = input.iter().rev().take_while(|&&byte| byte == b'=').count();
    let length = input.len() - padding;
    let remainder = length % 4;
    if padding > 2
        || remainder == 1
        || (padding != 0 && (!input.len().is_multiple_of(4) || padding != 4 - remainder))
    {
        return Err(Error::InvalidBase64);
    }
    let needed = length / 4 * 3
        + match remainder {
            2 => 1,
            3 => 2,
            _ => 0,
        };
    if needed > output.len() {
        return Err(Error::BufferTooSmall);
    }
    let mut written = 0;
    let (chunks, tail) = input[..length].as_chunks::<4>();
    for chunk in chunks {
        let [a, b, c, d] = [
            DECODE[chunk[0] as usize],
            DECODE[chunk[1] as usize],
            DECODE[chunk[2] as usize],
            DECODE[chunk[3] as usize],
        ];
        if (a | b | c | d) == INVALID {
            return Err(Error::InvalidBase64);
        }
        output[written] = a << 2 | b >> 4;
        output[written + 1] = b << 4 | c >> 2;
        output[written + 2] = c << 6 | d;
        written += 3;
    }
    if tail.len() >= 2 {
        let a = DECODE[tail[0] as usize];
        let b = DECODE[tail[1] as usize];
        if (a | b) == INVALID {
            return Err(Error::InvalidBase64);
        }
        output[written] = a << 2 | b >> 4;
        if tail.len() == 2 {
            if b & 15 != 0 {
                return Err(Error::InvalidBase64);
            }
        } else {
            let c = DECODE[tail[2] as usize];
            if c == INVALID || c & 3 != 0 {
                return Err(Error::InvalidBase64);
            }
            output[written + 1] = b << 4 | c >> 2;
        }
    }
    Ok(needed)
}
