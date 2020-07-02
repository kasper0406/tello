mod crc;
mod player;

extern crate gstreamer as gst;
extern crate gstreamer_app as gst_app;

use gst::prelude::*;

use std::net::{ SocketAddr, UdpSocket };
use std::convert::TryInto;
use std::thread;
use std::time;
use std::sync::Arc;
use std::sync::atomic::{ AtomicBool, Ordering };
use std::slice;
use std::assert;
use std::time::Duration;

use std::sync::mpsc::{ channel };

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
struct FlightData {
    height: u16,
    battery_percentage: u8,
    camera_state: u8
}

impl FlightData {
    fn from(bytes: &[u8]) -> FlightData {
        assert!(bytes.len() == 24);

        FlightData {
            height: (bytes[0] as u16) | ((bytes[1] as u16) << 8),
            battery_percentage: bytes[12],
            camera_state: bytes[20]
        }
    }
}

#[derive(Debug)]
enum TelloGramDirection {
    ToDrone, FromDrone, Unknown
}

impl TelloGram {
    const GRAM_SIZE: usize = 11;

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
        let payload_size = self.size() - TelloGram::GRAM_SIZE;
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

    fn construct_package(packet_type: u8, command: u16, seq: u16, payload: &[u8]) -> Vec<u8> {
        let packet_size = TelloGram::GRAM_SIZE + payload.len();

        let mut buffer = vec![0; packet_size];
        let mut gram = unsafe { &mut *buffer.as_mut_ptr().cast::<TelloGram>() };
        gram.m_header = 0xcc;
        gram.m_size = (packet_size << 3) as u16;
        gram.m_crc8 = crc::calculate_crc8(&buffer[..3]);
        gram.m_discriminator |= 0x40;
        gram.m_discriminator |= (packet_type << 3) & 0x38;
        // gram.m_discriminator |= packet_subtype & 0x7;
        gram.m_id = command;
        gram.m_sequence = seq;

        for i in 0..payload.len() {
            buffer[i + 9] = payload[i];
        }

        let crc16 = crc::calculate_crc16(&buffer[..packet_size - 2]);
        let crc16_buf: [u8; 2] = unsafe { std::mem::transmute(crc16.to_be()) };
        buffer[packet_size - 2] = crc16_buf[1];
        buffer[packet_size - 1] = crc16_buf[0];

        buffer
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
        const COLON: u8 = ':' as u8;
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
    gst::init().expect("Failed to init gstreamer");

    let is_running = Arc::new(AtomicBool::new(true));

    let cmd_bind_addr = SocketAddr::from(([0, 0, 0, 0], LOCAL_CMD_PORT));
    let cmd_socket_write = UdpSocket::bind(cmd_bind_addr).expect("Unable to create UDP command socket");
    cmd_socket_write.connect(SocketAddr::from((TELLO_IP, TELLO_CMD_PORT))).expect("Failed to connect to Tello command");

    let cmd_socket_read = cmd_socket_write.try_clone().expect("Failed to clone socket");
    cmd_socket_read.set_read_timeout(Some(Duration::from_secs(1))).expect("Failed to set cmd read timeout");

    let video_socket = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], VIDEO_PORT))).expect("Failed to create video socket");

    let cmd_listen_thread_running = is_running.clone();
    let cmd_listen_thread = thread::spawn(move || {
        let mut buffer: [u8; 4096] = [0; 4096];
        
        while (*cmd_listen_thread_running).load(Ordering::Relaxed) {
            match cmd_socket_read.recv(&mut buffer) {
                Ok(num_bytes) => {
                    // println!("Command package of {} bytes: {:?}", num_bytes, &buffer[..num_bytes]);

                    if buffer.starts_with("conn_ack:".as_bytes()) {
                        println!("Connected to Tello!");
                    } else {
                        // Interpret as TelloGram
                        let gram = unsafe { &*buffer.as_ptr().cast::<TelloGram>() };

                        if !gram.is_valid() {
                            println!("Received invalid TelloGram {:?}", &buffer[..num_bytes]);
                            continue
                        }

                        match gram.id() {
                            0x2 => {
                                println!("Connected");
                            },
                            0x56 => {
                                let data = FlightData::from(&gram.payload());
                                println!("{:?}", data);
                            },
                            _ => {
                                println!("Unhandled package type {}", gram.id());
                            }
                        }

                        /*
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
                        */
                    }
                },
                Err(e) => println!("receive failed: {:?}", e),
            }
        }
    });

    let pipeline = gst::Pipeline::new(None);
    let source = gst::ElementFactory::make("appsrc", None).expect("Failed to create appsource");
    let h264parse = gst::ElementFactory::make("h264parse", None).expect("Failed to create h264parse");
    let avdec_h264 = gst::ElementFactory::make("avdec_h264", None).expect("Failed to create avdec_h264");
    let videoconvert = gst::ElementFactory::make("videoconvert", None).expect("Failed to create videoconvert");
    let sink = gst::ElementFactory::make("appsink", None).expect("Failed to create appsink");

    pipeline.add_many(&[&source, &h264parse, &avdec_h264, &videoconvert, &sink]).expect("Failed to create pipeline");
    source.link(&h264parse).expect("Failed to link");
    h264parse.link(&avdec_h264).expect("Failed to link");
    avdec_h264.link(&videoconvert).expect("Failed to link");
    videoconvert.link(&sink).expect("Failed to link");

    let appsource = source.dynamic_cast::<gst_app::AppSrc>().expect("Pipeline should be an appsource!");
    let appsink = sink.dynamic_cast::<gst_app::AppSink>().expect("Pipeline should be an appsink!");

    appsource.set_latency(gst::ClockTime::from_mseconds(0), gst::ClockTime::from_mseconds(10));
    appsource.set_property_is_live(true);
    appsource.set_stream_type(gst_app::AppStreamType::Stream);

    appsink.set_caps(Some(&gst::Caps::new_simple(
        "video/x-raw",
        &[
            ("format", &"RGBA")
        ]
    )));

    pipeline.set_state(gst::State::Playing).expect("Failed to change pipeline state to play");

    let video_listen_thread_running = is_running.clone();
    let video_listen_thread = thread::spawn(move || {
        let mut buffer = [0; 4096];            
        while (*video_listen_thread_running).load(Ordering::Relaxed) {
            match video_socket.recv(&mut buffer) {
                Ok(num_bytes) => {
                    let mut databuf = vec![0; num_bytes - 2];
                    for i in 0..num_bytes - 2 {
                        databuf[i] = buffer[i + 2];
                    }

                    appsource.push_buffer(gst::buffer::Buffer::from_slice(databuf)).expect("Failed to push vidoe buffer");
                },
                Err(e) => println!("Failed to receive video buffer: {}", e)
            }
        }
    });

    let video_processor_thread_running = is_running.clone();
    let (video_sender, video_receiver) = channel();
    let video_processor_thread = thread::spawn(move || {
        while (*video_processor_thread_running).load(Ordering::Relaxed) {
            match appsink.try_pull_sample(gst::ClockTime::from_seconds(1)) {
                Some(sample) => {
                    // TODO: Get width, height from Sample::caps

                    let buffer = sample.get_buffer().unwrap();
                    let mut data = vec![0; buffer.get_size()];
                    buffer.copy_to_slice(0, &mut data).unwrap();

                    video_sender.send(player::Frame {
                        width: 960,
                        height: 720,
                        data: data
                    }).expect("Failed to send frame");
                },
                None => ()
            }
        }
    });

    
    let player = player::Player::new(video_receiver);
    let connect_request = TelloConnectRequest::connect(VIDEO_PORT);

    println!("Sending bytes to Tello {:?}", connect_request.as_bytes().as_slice());
    cmd_socket_write.send(connect_request.as_bytes().as_slice()).expect("Failed to send command to Tello");

    let video_ping_thread_running = is_running.clone();
    let video_package_ping_thread = thread::spawn(move || {
        while (*video_ping_thread_running).load(Ordering::Relaxed) {
            let spspps_video_req = TelloGram::construct_package(4, 0x25, 0, &[]);
            cmd_socket_write.send(&spspps_video_req).expect("Failed to send video request");
            thread::sleep(time::Duration::from_millis(2000));
        }
    });

    player.run();

    is_running.store(false, Ordering::Relaxed);

    video_package_ping_thread.join().expect("Failed to join video ping thread");
    video_listen_thread.join().expect("Failed to join video listener thread");
    video_processor_thread.join().expect("Failed to join video processor thread");
    cmd_listen_thread.join().expect("Failed to join cmd thread");
}
