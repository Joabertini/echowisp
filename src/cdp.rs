// Cliente WebSocket minimo (RFC 6455) para hablar CDP con Brave en 127.0.0.1.
// Lectura bloqueante en un hilo; escritura desde cualquier hilo via Mutex.
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::TcpStream,
    sync::Mutex,
};

pub struct Ws {
    w: Mutex<TcpStream>,
}

pub fn connect(port: u16, path: &str) -> io::Result<(Ws, BufReader<TcpStream>)> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_nodelay(true)?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
Sec-WebSocket-Key: eXRtLWZsb2F0LWtleQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )?;
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    if !line.contains(" 101 ") {
        return Err(io::Error::other(format!("handshake: {}", line.trim())));
    }
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
    }
    Ok((Ws { w: Mutex::new(s) }, r))
}

impl Ws {
    pub fn send(&self, text: &str) -> io::Result<()> {
        self.frame(0x1, text.as_bytes())
    }

    fn frame(&self, op: u8, data: &[u8]) -> io::Result<()> {
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
        let key = [0x5a, 0x3c, 0x96, 0xe1];
        f.extend_from_slice(&key);
        f.extend(data.iter().enumerate().map(|(i, b)| b ^ key[i & 3]));
        self.w.lock().unwrap().write_all(&f)
    }
}

/// Devuelve el proximo mensaje de texto completo; responde pings solo.
pub fn read_msg(r: &mut BufReader<TcpStream>, ws: &Ws) -> io::Result<String> {
    let mut msg = Vec::new();
    loop {
        let mut h = [0u8; 2];
        r.read_exact(&mut h)?;
        let fin = h[0] & 0x80 != 0;
        let op = h[0] & 0x0f;
        let mut n = (h[1] & 0x7f) as u64;
        if n == 126 {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            n = u16::from_be_bytes(b) as u64;
        } else if n == 127 {
            let mut b = [0u8; 8];
            r.read_exact(&mut b)?;
            n = u64::from_be_bytes(b);
        }
        let mut data = vec![0u8; n as usize];
        r.read_exact(&mut data)?;
        match op {
            0x8 => return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "ws cerrado")),
            0x9 => ws.frame(0xA, &data)?,
            0xA => {}
            _ => {
                msg.extend_from_slice(&data);
                if fin {
                    return String::from_utf8(msg).map_err(io::Error::other);
                }
            }
        }
    }
}
