#![allow(non_snake_case, dead_code)]

extern crate bn;
extern crate rand;
extern crate snark;
extern crate crossbeam;
extern crate rustc_serialize;
extern crate bincode;

#[macro_use]
extern crate log;
extern crate env_logger;
extern crate time;
extern crate ansi_term;

mod multicore;
mod qap;
mod protocol;
mod spair;

use snark::*;
use self::protocol::*;
use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use rustc_serialize::{Decodable, Encodable};
use rustc_serialize::hex::ToHex;
use bincode::SizeLimit::Infinite;
use bincode::rustc_serialize::{encode_into, decode_from};
use std::time::Duration;

const LISTEN_ADDR: &'static str = "0.0.0.0:65530";
const NETWORK_MAGIC: [u8; 8] = [0xff, 0xff, 0x1f, 0xbb, 0x1c, 0xee, 0x00, 0x19];
const PLAYERS: usize = 3;
pub const THREADS: usize = 128;

#[derive(Clone)]
struct ConnectionHandler {
    peers: Arc<Mutex<HashMap<[u8; 8], Option<TcpStream>>>>,
    notifier: Sender<[u8; 8]>
}

impl ConnectionHandler {
    fn new() -> ConnectionHandler {
        let (tx, rx) = channel();

        let handler = ConnectionHandler {
            peers: Arc::new(Mutex::new(HashMap::new())),
            notifier: tx
        };

        {
            let handler = handler.clone();
            thread::spawn(move || {
                handler.run(rx);
            });
        }

        handler
    }

    fn do_with_stream<T, E, F: Fn(&mut TcpStream) -> Result<T, E>>(&self, peerid: &[u8; 8], cb: F) -> T
    {
        let waittime = Duration::from_secs(5);

        loop {
            // The stream is always there, because we put it back
            // even if it fails.
            let mut stream: TcpStream = {
                let mut peers = self.peers.lock().unwrap();
                peers.get_mut(peerid).unwrap().take()
            }.unwrap();

            let val = cb(&mut stream);

            {
                // Put it back
                let mut peers = self.peers.lock().unwrap();
                *peers.get_mut(peerid).unwrap() = Some(stream);
            }

            match val {
                Err(_) => {
                    thread::sleep(waittime);
                },
                Ok(v) => {
                    return v
                }
            }
        }
    }

    fn read<T: Decodable>(&self, peerid: &[u8; 8]) -> T
    {
        self.do_with_stream(peerid, |s| decode_from(s, Infinite))
    }

    fn write<T: Encodable>(&self, peerid: &[u8; 8], obj: &T)
    {
        self.do_with_stream(peerid, |s| encode_into(obj, s, Infinite))
    }

    fn run(&self, new_peers: Receiver<[u8; 8]>)
    {
        info!("Loading R1CS from disk and performing QAP reduction...");

        let cs = CS::from_file();

        info!("Waiting for players to connect...");

        let mut peers = vec![];
        let mut pubkeys = vec![];
        let mut commitments: Vec<[u8; 32]> = vec![];
        for peerid in new_peers.into_iter().take(PLAYERS) {
            info!("Initializing new player (peerid={})", peerid.to_hex());
            info!("Asking for commitment to PublicKey (peerid={})", peerid.to_hex());
            commitments.push(self.read(&peerid));
            info!("PublicKey Commitment received (peerid={})", peerid.to_hex());
            peers.push(peerid);
        }

        // The remote end should never hang up, so this should always be `PLAYERS`.
        assert_eq!(peers.len(), PLAYERS);

        info!("All players are ready");

        info!("Initializing stage1 with constraint system");

        let mut stage1 = Stage1Contents::new(&cs);
        for (comm, peerid) in commitments.iter().zip(peers.iter()) {
            info!("Sending stage1 to peerid={}", peerid.to_hex());

            self.write(peerid, &stage1);

            info!("Receiving stage1 transformation from peerid={}", peerid.to_hex());

            // TODO: verify pubkey against comm
            let pubkey = self.read::<PublicKey>(peerid);
            let new_stage1 = self.read::<Stage1Contents>(peerid);

            info!("Verifying transformation of stage1 from peerid={}", peerid.to_hex());

            if !new_stage1.verify_transform(&stage1, &pubkey) {
                error!("Peer did not perform valid stage1 transformation (peerid={})", peerid.to_hex());
                panic!("cannot recover.");
            } else {
                pubkeys.push(pubkey);
                stage1 = new_stage1;
            }
        }

        info!("Initializing stage2 with constraint system and stage1");

        let mut stage2 = Stage2Contents::new(&cs, &stage1);
        for (pubkey, peerid) in pubkeys.iter().zip(peers.iter()) {
            info!("Sending stage2 to peerid={}", peerid.to_hex());

            self.write(peerid, &stage2);

            info!("Receiving stage2 transformation from peerid={}", peerid.to_hex());

            let new_stage2 = self.read::<Stage2Contents>(peerid);

            info!("Verifying transformation of stage2 from peerid={}", peerid.to_hex());

            if !new_stage2.verify_transform(&stage2, pubkey) {
                error!("Peer did not perform valid stage2 transformation (peerid={})", peerid.to_hex());
                panic!("cannot recover.");
            } else {
                stage2 = new_stage2;
            }
        }

        info!("Initializing stage3 with stage2");

        let mut stage3 = Stage3Contents::new(&stage2);
        for (pubkey, peerid) in pubkeys.iter().zip(peers.iter()) {
            info!("Sending stage3 to peerid={}", peerid.to_hex());

            self.write(peerid, &stage3);

            info!("Receiving stage3 transformation from peerid={}", peerid.to_hex());

            let new_stage3 = self.read::<Stage3Contents>(peerid);

            info!("Verifying transformation of stage3 from peerid={}", peerid.to_hex());

            if !new_stage3.verify_transform(&stage3, pubkey) {
                error!("Peer did not perform valid stage3 transformation (peerid={})", peerid.to_hex());
                panic!("cannot recover.");
            } else {
                stage3 = new_stage3;
            }
        }

        info!("MPC complete, producing keypair.");

        let kp = keypair(&cs, &stage1, &stage2, &stage3);

        kp.write_to_disk();

        info!("Keypair written to disk.");
    }

    fn accept(&self, peerid: [u8; 8], stream: TcpStream) {
        use std::collections::hash_map::Entry::{Occupied, Vacant};

        let mut peers = self.peers.lock().unwrap();

        match peers.entry(peerid) {
            Occupied(mut already) => {
                if already.get().is_none() {
                    warn!("Ignoring duplicate connection attempt (peerid={})", peerid.to_hex());
                } else {
                    warn!("Re-established connection with peerid={}", peerid.to_hex());
                    already.insert(Some(stream));
                }
            },
            Vacant(vacant) => {
                match self.notifier.send(peerid) {
                    Ok(_) => {
                        info!("Accepted new connection (peerid={})", peerid.to_hex());
                        vacant.insert(Some(stream));
                    },
                    Err(_) => {
                        warn!("Rejecting connection from peerid={}, no longer accepting new players.", peerid.to_hex());
                    }
                }
            }
        }
    }
}

fn main() {
    {
        // Initialize the logger.
        let start_time = time::now();
        let format = move |record: &log::LogRecord| {
            use ansi_term::Colour::*;

            let since = time::now() - start_time;
            let hours = since.num_hours();
            let minutes = since.num_minutes() % 60;
            let seconds = since.num_seconds() % 60;

            let level = match record.level() {
                a @ log::LogLevel::Warn => {
                    format!("{}", Yellow.bold().paint(format!("{}", a)))
                },
                a @ log::LogLevel::Error => {
                    format!("{}", Red.bold().paint(format!("{}", a)))
                },
                a @ _ => {
                    format!("{}", a)
                }
            };

            format!("({}) [T+{}h{}m{}s]: {}", level, hours, minutes, seconds, record.args())
        };

        let mut builder = env_logger::LogBuilder::new();
        builder.format(format).filter(None, log::LogLevelFilter::Info);
        builder.init().unwrap();
    }

    info!("Opening TCP listener on {}", LISTEN_ADDR);
    let listener = TcpListener::bind(LISTEN_ADDR).unwrap();

    let handler = ConnectionHandler::new();

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                stream.set_read_timeout(Some(Duration::from_secs(5)));
                stream.set_write_timeout(Some(Duration::from_secs(5)));

                match stream.peer_addr() {
                    Ok(addr) => {
                        let mut magic = [0; 8];
                        let mut peerid = [0; 8];

                        match stream.read_exact(&mut magic)
                                    .and_then(|_| stream.read_exact(&mut peerid))
                        {
                            Err(e) => {
                                warn!("Remote host {} did not handshake; {}", addr, e);
                            },
                            Ok(_) => {
                                if magic != NETWORK_MAGIC {
                                    warn!("Remote host {} did not supply correct network magic.", addr);
                                } else {
                                    info!("Establishing connection with {} (peerid={})", addr, peerid.to_hex());
                                    stream.set_read_timeout(Some(Duration::from_secs(60)));
                                    stream.set_write_timeout(Some(Duration::from_secs(60)));
                                    handler.accept(peerid, stream);
                                }
                            }
                        }
                    },
                    Err(e) => {
                        warn!("Failed connection attempt from unknown host: {}", e);
                    }
                }
            },
            Err(e) => {
                warn!("Failed to establish connection with remote client, {}", e);
            }
        }
    }
}