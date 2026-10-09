// Cliente WebSocket minimo (RFC 6455) para hablar CDP con Brave en 127.0.0.1.
// Lectura bloqueante en un hilo; escritura desde cualquier hilo via Mutex.
// Protocolo: https://www.rfc-editor.org/rfc/rfc6455#section-4.2.2 y #section-5.2.
// Aleatorio y hash: BCryptGenRandom/BCryptHash (Windows CNG); SHA-1 aqui es obligatorio por RFC 6455.
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::TcpStream,
    ptr::null_mut,
    sync::Mutex,
    time::Duration,
};
use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCryptHash, BCRYPT_SHA1_ALG_HANDLE, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};

const MAX_MESSAGE: usize = 16 * 1024 * 1024;
const SOCKET_TIMEOUT: Duration = Duration::from_secs(30);
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub struct Ws {
    w: Mutex<TcpStream>,
}

fn random<const N: usize>() -> io::Result<[u8; N]> {
    let mut bytes = [0u8; N];
    let status = unsafe {
        BCryptGenRandom(
            null_mut(),
            bytes.as_mut_ptr(),
            N as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(io::Error::other(format!("BCryptGenRandom: {status:#x}")));
    }
    Ok(bytes)
}

fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(((data.len() + 2) / 3) * 4);
    for chunk in data.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        out.push(BASE64[(a >> 2) as usize] as char);
        out.push(BASE64[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            BASE64[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn expected_accept(key: &str) -> io::Result<String> {
    let input = format!("{key}{GUID}");
    let mut hash = [0u8; 20];
    let status = unsafe {
        BCryptHash(
            BCRYPT_SHA1_ALG_HANDLE,
            null_mut(),
            0,
            input.as_ptr(),
            input.len() as u32,
            hash.as_mut_ptr(),
            hash.len() as u32,
        )
    };
    if status != 0 {
        return Err(io::Error::other(format!("BCryptHash SHA-1: {status:#x}")));
    }
    Ok(base64(&hash))
}

fn verify_accept(key: &str, received: Option<&str>) -> io::Result<()> {
    if received != Some(expected_accept(key)?.as_str()) {
        return Err(protocol_error("Sec-WebSocket-Accept incorrecto"));
    }
    Ok(())
}

fn protocol_error(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub fn connect(port: u16, path: &str) -> io::Result<(Ws, BufReader<TcpStream>)> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_nodelay(true)?;
    s.set_read_timeout(Some(SOCKET_TIMEOUT))?;
    s.set_write_timeout(Some(SOCKET_TIMEOUT))?;
    let key = base64(&random::<16>()?);
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )?;
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    if !line.starts_with("HTTP/1.1 101 ") && !line.starts_with("HTTP/1.0 101 ") {
        return Err(protocol_error(&format!("handshake: {}", line.trim())));
    }
    let mut accept = None;
    let mut header_bytes = line.len();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return Err(protocol_error("handshake incompleto"));
        }
        header_bytes += line.len();
        if header_bytes > 16 * 1024 {
            return Err(protocol_error("handshake demasiado grande"));
        }
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Sec-WebSocket-Accept") {
                if accept.replace(value.trim().to_string()).is_some() {
                    return Err(protocol_error("Sec-WebSocket-Accept duplicado"));
                }
            }
        }
    }
    verify_accept(&key, accept.as_deref())?;
    // El timeout de lectura cubre solo el handshake: después el bucle de eventos espera sin límite
    // (en pausa puede no llegar nada por minutos; con 30 s la sesión se reiniciaba, os error 10060).
    s.set_read_timeout(None)?;
    r.get_ref().set_read_timeout(None)?;
    Ok((Ws { w: Mutex::new(s) }, r))
}

impl Ws {
    pub fn send(&self, text: &str) -> io::Result<()> {
        self.frame(0x1, text.as_bytes())
    }

    fn frame(&self, op: u8, data: &[u8]) -> io::Result<()> {
        if data.len() > MAX_MESSAGE {
            return Err(protocol_error("frame saliente demasiado grande"));
        }
        let key = random::<4>()?;
        let mut f = Vec::with_capacity(data.len() + 14);
        f.push(0x80 | op);
        let n = data.len();
        if n < 126 {
            f.push(0x80 | n as u8);
        } else if n < 65536 {
            f.push(0x80 | 126);
            f.extend_from_slice(&(n as u16).to_be_bytes());
        } else {
            f.push(0x80 | 127);
            f.extend_from_slice(&(n as u64).to_be_bytes());
        }
        f.extend_from_slice(&key);
        f.extend(data.iter().enumerate().map(|(i, b)| b ^ key[i & 3]));
        self.w.lock().unwrap().write_all(&f)
    }
}

/// Devuelve el proximo mensaje de texto completo y responde pings con pong.
pub fn read_msg(r: &mut BufReader<TcpStream>, ws: &Ws) -> io::Result<String> {
    read_msg_from(r, |data| ws.frame(0xA, data))
}

fn read_msg_from<R: Read>(
    r: &mut R,
    mut pong: impl FnMut(&[u8]) -> io::Result<()>,
) -> io::Result<String> {
    let mut msg = Vec::new();
    let mut fragmented = false;
    loop {
        let mut h = [0u8; 2];
        r.read_exact(&mut h)?;
        if h[0] & 0x70 != 0 || h[1] & 0x80 != 0 {
            return Err(protocol_error(
                "bits reservados o frame de servidor enmascarado",
            ));
        }
        let fin = h[0] & 0x80 != 0;
        let op = h[0] & 0x0f;
        if !matches!(op, 0x0 | 0x1 | 0x2 | 0x8 | 0x9 | 0xA) {
            return Err(protocol_error("opcode WebSocket invalido"));
        }
        let mut n = (h[1] & 0x7f) as u64;
        if n == 126 {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            n = u16::from_be_bytes(b) as u64;
        } else if n == 127 {
            let mut b = [0u8; 8];
            r.read_exact(&mut b)?;
            n = u64::from_be_bytes(b);
            if b[0] & 0x80 != 0 {
                return Err(protocol_error("longitud WebSocket invalida"));
            }
        }
        if op >= 0x8 && (!fin || n > 125) {
            return Err(protocol_error("frame de control WebSocket invalido"));
        }
        if n > MAX_MESSAGE as u64 || (op <= 0x2 && n > (MAX_MESSAGE - msg.len()) as u64) {
            return Err(protocol_error("mensaje WebSocket demasiado grande"));
        }
        if op == 0x2 {
            return Err(protocol_error("mensaje WebSocket binario inesperado"));
        }
        if (op == 0x0 && !fragmented) || (op == 0x1 && fragmented) {
            return Err(protocol_error("fragmentacion WebSocket invalida"));
        }
        let mut data = vec![0u8; n as usize];
        r.read_exact(&mut data)?;
        match op {
            0x8 => {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "ws cerrado",
                ))
            }
            0x9 => pong(&data)?,
            0xA => {}
            0x0 | 0x1 => {
                msg.extend_from_slice(&data);
                if fin {
                    return String::from_utf8(msg).map_err(io::Error::other);
                }
                fragmented = true;
            }
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_rfc_6455() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let accept = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";
        assert_eq!(expected_accept(key).unwrap(), accept);
        assert!(verify_accept(key, Some(accept)).is_ok());
        assert!(verify_accept(key, Some("accept incorrecto")).is_err());
        assert!(verify_accept(key, None).is_err());
    }

    #[test]
    fn frame_demasiado_grande() {
        let mut frame = vec![0x81, 127];
        frame.extend_from_slice(&((MAX_MESSAGE as u64) + 1).to_be_bytes());
        let err = read_msg_from(&mut frame.as_slice(), |_| Ok(())).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("demasiado grande"));
    }

    #[test]
    fn opcode_invalido() {
        let err = read_msg_from(&mut [0x83, 0].as_slice(), |_| Ok(())).unwrap_err();
        assert!(err.to_string().contains("opcode"));
    }

    #[test]
    fn ping_pong_y_fragmentos() {
        let frames = [
            0x01, 2, b'h', b'o', 0x89, 2, b'o', b'k', 0x80, 2, b'l', b'a',
        ];
        let mut pongs = Vec::new();
        let msg = read_msg_from(&mut frames.as_slice(), |data| {
            pongs.push(data.to_vec());
            Ok(())
        })
        .unwrap();
        assert_eq!(msg, "hola");
        assert_eq!(pongs, [b"ok".to_vec()]);
    }
}
