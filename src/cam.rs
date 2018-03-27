use std::{thread, error, str};

use rscam::{Camera, Config, FormatInfo, ResolutionInfo};

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use demosaic::demosaic;

const BP: [u8; 3] = [0, 0, 255];

pub struct Cam {
    camera: Camera,
    resolution: (u32, u32),
    interval: (u32, u32),
    format: [u8; 4],
    frame_size: usize,
    pixels: usize,
}

pub struct FrameBuf {
    pub buf: Vec<[u8; 3]>,
    pub ts: u64,
}

impl Cam {
    pub fn new(dev: &str) -> Result<Self, Box<error::Error>> {
        let mut camera = Camera::new(dev)
            .map_err(Box::new)?;

        let format_info = camera.formats().find(|v| {
            match *v {
                Ok(FormatInfo{ ref format, .. }) => {
                    format == b"YUYV" || format == b"RGGB" || format == b"GREY"
                }
                Err(ref e) => panic!("{:?}", e),
            }
        }).expect("camera does not have supported formats")?;

        let format = format_info.format;

        let resolution = match camera.resolutions(&format)? {
            ResolutionInfo::Discretes(v) => *v.iter().max().unwrap(),
            ResolutionInfo::Stepwise{max, ..} => max,
        };
        //let resolution = (640, 480);

        let frame_size = match &format {
            b"YUYV" => 2*resolution.0*resolution.1,
            b"GREY" | b"RGGB" => resolution.0*resolution.1,
            _ => unreachable!(),
        } as usize;

        /*
        let interval = match camera.intervals(&format, resolution)? {
            IntervalInfo::Discretes(v) => *v.iter().filter(|v| v.1 <= 60).max().unwrap(),
            IntervalInfo::Stepwise{max, ..} => max,
        };
        */
        let interval = (1, 30);

        camera.start(&Config {
            interval,
            resolution,
            format: &format,
            ..Default::default()
        }).map_err(Box::new)?;

        let pixels = (resolution.0*resolution.1) as usize;
        Ok(Cam {camera, resolution, interval, format, frame_size, pixels})
    }

    pub fn run_worker(self, pause: Arc<AtomicBool>)
        -> Arc<Mutex<FrameBuf>>
    {
        let cam_mutex = Arc::new(Mutex::new(FrameBuf {
            buf: vec![BP; self.pixels],
            ts: 0,
        }));
        let mutex = cam_mutex.clone();

        thread::spawn(move|| {
            let mut prev = 0u64;

            loop {
                let frame = self.camera.capture()
                    .expect("failed to capture camera frame");

                if pause.load(Ordering::Relaxed) { continue; }

                let t = frame.get_timestamp();

                if is_drop(t, prev, self.interval) {
                    //println!("Frame drop");
                }

                let mut guard = mutex.lock().unwrap();
                if frame.len() == self.frame_size {
                    demosaic(&self, &mut guard.buf, &frame);
                } else {
                    println!("Bad frame len: {}", frame.len());
                    guard.buf.iter_mut().for_each(|p| *p = BP);
                };

                guard.ts = t;
                prev = t;
            }
        });
        cam_mutex
    }

    pub fn get_resolution(&self) -> (u32, u32) {
        self.resolution
    }

    pub fn get_interval(&self) -> (u32, u32) {
        self.interval
    }

    pub fn get_format(&self) -> [u8; 4] {
        self.format
    }

    pub fn get_pixels(&self) -> usize {
        self.pixels
    }

    pub fn get_frame_size(&self) -> usize {
        self.frame_size
    }
}

fn is_drop(t: u64, prev: u64, interval: (u32, u32)) -> bool {
    let dt = t - prev;
    //println!("{:?} {}", dt, (interval.0 as u64)*1_000_000/(interval.1 as u64));
    dt > u64::from(interval.0)*1_100_000/u64::from(interval.1)
}
