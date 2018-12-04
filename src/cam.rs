use std::{thread, error, str};

use rscam::{Camera, Config, ResolutionInfo, IntervalInfo};

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use demosaic::demosaic;

const BP: [u8; 3] = [0, 0, 255];
const MAX_FPS: u32 = 60;

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
    pub hist: [u32; 256],
}

impl Cam {
    pub fn new(dev: &str) -> Result<Self, Box<error::Error>> {
        let mut camera = Camera::new(dev)
            .map_err(Box::new)?;

        let mut format = Err("camera formats are not supported".to_string());
        let mut format_priority = 0u8;
        eprint!("Camera formats:");
        for f in camera.formats() {
            let f = f?;
            eprint!(" {}{}{}{}",
                f.format[0] as char, f.format[1] as char,
                f.format[2] as char, f.format[3] as char,
            );
            let priority = match &f.format {
                b"YUYV" => 1,
                b"GREY" => 2,
                b"RGGB" => 3,
                b"BGR3" => 4,
                b"RGB3" => 5,
                _ => continue,
            };
            if format_priority < priority {
                format = Ok(f.format);
                format_priority = priority;
            }
        }
        eprintln!();
        let format = format?;
        eprintln!("Selected format: {}{}{}{}",
            format[0] as char, format[1] as char,
            format[2] as char, format[3] as char,
        );

        let resolution = match camera.resolutions(&format)? {
            ResolutionInfo::Discretes(v) => *v.iter().max().unwrap(),
            ResolutionInfo::Stepwise{max, ..} => max,
        };
        eprintln!("Selected resolution: {:?}", resolution);

        let frame_size = match &format {
            b"YUYV" => 2*resolution.0*resolution.1,
            b"GREY" | b"RGGB" => resolution.0*resolution.1,
            b"BGR3" | b"RGB3" => 3*resolution.0*resolution.1,
            _ => unreachable!(),
        } as usize;

        let interval = match camera.intervals(&format, resolution)? {
            IntervalInfo::Discretes(v) =>
                *v.iter().filter(|v| v.0 == 1 && v.1 <= MAX_FPS).max().unwrap(),
            IntervalInfo::Stepwise{max, ..} => max,
        };
        eprintln!("Selected interval: {:?}", interval);

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
            hist: [0; 256],
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
                guard.hist = calc_hist(&guard.buf);
                prev = t;
            }
        });
        cam_mutex
    }

    pub fn get_resolution(&self) -> [u32; 2] {
        [self.resolution.0, self.resolution.1]
    }

    /*pub fn get_interval(&self) -> (u32, u32) {
        self.interval
    }
    */

    pub fn get_format(&self) -> [u8; 4] {
        self.format
    }

    pub fn get_pixels(&self) -> usize {
        self.pixels
    }

    pub fn get_frame_size(&self) -> usize {
        self.frame_size
    }

    pub fn is_grey(&self) -> bool {
        &self.get_format() == b"GREY"
    }
}

fn is_drop(t: u64, prev: u64, interval: (u32, u32)) -> bool {
    let dt = t - prev;
    //println!("{:?} {}", dt, (interval.0 as u64)*1_000_000/(interval.1 as u64));
    dt > u64::from(interval.0)*1_100_000/u64::from(interval.1)
}

fn calc_hist(buf: &[[u8; 3]]) -> [u32; 256] {
    let mut hist = [0u32; 256];
    for b in buf {
        let i = ((b[0] as usize) + 2*(b[1] as usize) + (b[2] as usize))/4;
        // safe because we guarantee that i <= 255
        unsafe { *hist.get_unchecked_mut(i) += 1; }
    }
    hist
}
