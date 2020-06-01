mod crc;

use std::net::{ SocketAddr, UdpSocket };
use std::convert::TryInto;
use std::thread;
use std::time;
use std::sync::Arc;
use std::sync::atomic::{ AtomicBool, Ordering };
use std::slice;
use std::assert;

const TELLO_CMD_PORT: u16 = 8889;
const LOCAL_CMD_PORT: u16 = 8800;
const VIDEO_PORT: u16 = 8040;
const TELLO_IP: [u8; 4] = [ 192, 168, 10, 1 ];

#[repr(packed(1))]
struct TelloGram {
    m_header: u8,
    m_size: u16,
    m_crc8: u8,
    m_discriminator: u8,
    m_id: u16,
    m_sequence: u16
}

#[derive(Debug)]
enum TelloGramDirection {
    ToDrone, FromDrone, Unknown
}

impl TelloGram {
    fn header(&self) -> u8 {
        self.m_header
    }

    fn size(&self) -> usize {
        (self.m_size >> 3) as usize
    }

    fn crc8(&self) -> u8 {
        self.m_crc8
    }

    fn packet_direction(&self) -> TelloGramDirection {
        match self.m_discriminator {
            val if (val & 0x80) != 0 => TelloGramDirection::FromDrone,
            val if (val & 0x40) != 0 => TelloGramDirection::ToDrone,
            _ => TelloGramDirection::Unknown
        }
    }

    fn packet_type(&self) -> u8 {
        (self.m_discriminator >> 3) & 0x7
    }

    fn packet_subtype(&self) -> u8 {
        self.m_discriminator & 0x7
    }

    fn id(&self) -> u16 {
        self.m_id
    }

    fn sequence(&self) -> u16 {
        self.m_sequence
    }

    fn payload(&self) -> Vec<u8> {
        let payload_size = self.size() - 11;
        unsafe {
            let gram_start = (self as *const TelloGram) as *const u8;
            slice::from_raw_parts(gram_start.offset(9), payload_size)
        }.to_vec()
    }

    fn crc16(&self) -> u16 {
        unsafe {
            let crc16_offset = (self.size() as isize) - 2;
            let crc16_start = ((self as *const TelloGram) as *const u8).offset(crc16_offset);
            let mut res: u16 = *(crc16_start.offset(1)) as u16;
            res = res << 8;
            res |= *crc16_start as u16;
            res
        }
    }

    fn is_valid(&self) -> bool {
        let header_slice = unsafe {
            let gram_start = (self as *const TelloGram) as *const u8;
            slice::from_raw_parts(gram_start, 3)
        };
        let payload_slice = unsafe {
            let gram_start = (self as *const TelloGram) as *const u8;
            slice::from_raw_parts(gram_start, self.size() - 2)
        };
        return crc::calculate_crc8(header_slice) == self.crc8()
            && crc::calculate_crc16(payload_slice) == self.crc16();
    }
}

trait NetworkPackage {
    fn as_bytes(&self) -> Vec<u8>;
}

struct TelloConnectRequest<'a> {
    cmd: &'a str,
    video_port: u16
}

impl<'a> TelloConnectRequest<'a> {
    fn connect(video_port: u16) -> TelloConnectRequest<'a> {
        TelloConnectRequest {
            cmd: "conn_req",
            video_port
        }
    }
}

impl<'a> NetworkPackage for TelloConnectRequest<'a> {
    fn as_bytes(&self) -> Vec<u8> {
        const COLON: u8 = 0x3a; // ':' in ascii
        let command_bytes = self.cmd.as_bytes();
        let mut bytes: Vec<u8> = Vec::with_capacity(command_bytes.len() + 3);

        for byte in command_bytes { bytes.push(*byte) }
        bytes.push(COLON);
        bytes.push((self.video_port & 0xff).try_into().expect("Wtf"));
        bytes.push((self.video_port >> 8).try_into().expect("Wtf"));
        
        bytes
    }
}

fn main() {
    let is_running = Arc::new(AtomicBool::new(true));

    let cmd_bind_addr = SocketAddr::from(([0, 0, 0, 0], LOCAL_CMD_PORT));
    let cmd_socket_write = UdpSocket::bind(cmd_bind_addr).expect("Unable to create UDP command socket");
    cmd_socket_write.connect(SocketAddr::from((TELLO_IP, TELLO_CMD_PORT))).expect("Failed to connect to Tello command");

    let cmd_socket_read = cmd_socket_write.try_clone().expect("Failed to clone socket");

    let video_socket = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], VIDEO_PORT))).expect("Failed to create video socket");

    let cmd_listen_thread_running = is_running.clone();
    let cmd_listen_thread = thread::spawn(move || {
        let mut buffer: [u8; 4096] = [0; 4096];
        
        while (*cmd_listen_thread_running).load(Ordering::Relaxed) {
            match cmd_socket_read.recv(&mut buffer) {
                Ok(num_bytes) => {
                    println!("Command package of {} bytes: {:?}", num_bytes, &buffer[..num_bytes]);

                    if buffer.starts_with("conn_ack:".as_bytes()) {
                        println!("Connected to Tello!");
                    } else {
                        // Interpret as TelloGram
                        let gram = unsafe { &*buffer.as_ptr().cast::<TelloGram>() };

                        if !gram.is_valid() {
                            println!("Received invalid TelloGram {:?}", &buffer[..num_bytes]);
                            continue
                        }

                        println!("Header: {:?}", gram.header());
                        println!("Size: {:?}", gram.size());
                        println!("CRC8: {:?}", gram.crc8());
                        println!("Packet direction: {:?}", gram.packet_direction());
                        println!("Type: {:?}", gram.packet_type());
                        println!("Subtype: {:?}", gram.packet_subtype());
                        println!("Id: {:?}", gram.id());
                        println!("Sequence: {:?}", gram.sequence());
                        println!("CRC16: {:?}", gram.crc16());
                        println!("Payload: {:?}", gram.payload());
                        println!("");
                    }
                },
                Err(e) => println!("receive failed: {:?}", e),
            }
        }
    });

    let video_listen_thread_running = is_running.clone();
    let video_listen_thread = thread::spawn(move || {
        let mut buffer: [u8; 4096] = [0; 4096];
        
        while (*video_listen_thread_running).load(Ordering::Relaxed) {
            match video_socket.recv(&mut buffer) {
                Ok(num_bytes) => {
                    let video_bytes = &buffer[..num_bytes];
                    // println!("Video package of {} bytes", num_bytes);
                },
                Err(e) => println!("receive failed: {:?}", e),
            }
        }
    });

    let connect_request = TelloConnectRequest::connect(VIDEO_PORT);

    println!("Sending bytes to Tello {:?}", connect_request.as_bytes().as_slice());
    cmd_socket_write.send(connect_request.as_bytes().as_slice()).expect("Failed to send command to Tello");

    thread::sleep(time::Duration::from_secs(1));
    is_running.store(false, Ordering::Relaxed);

    video_listen_thread.join().expect("Failed to join video thread");
    cmd_listen_thread.join().expect("Failed to join cmd thread");
}
