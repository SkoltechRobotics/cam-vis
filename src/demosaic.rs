use std::io::Cursor;
use std::slice;
use bayer;

use cam::Cam;

fn demosaic_rggb(cam: &Cam, buf: &mut [[u8; 3]], frame: &[u8]) {
    assert_eq!(buf.len(), cam.get_pixels());
    assert_eq!(frame.len(), cam.get_frame_size());
    assert_eq!(frame.len(), buf.len());

    let buf2 = unsafe {
        slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, 3*buf.len())
    };

    let res = cam.get_resolution();
    let mut dst = bayer::RasterMut::new(
        res.0 as usize, res.1 as usize,
        bayer::RasterDepth::Depth8,
        buf2,
    );

    bayer::run_demosaic(
        &mut Cursor::new(frame),
        bayer::BayerDepth::Depth8,
        bayer::CFA::RGGB,
        bayer::Demosaic::Linear,
        &mut dst
    ).unwrap();
}

fn demosaic_yuyv(cam: &Cam, buf: &mut [[u8; 3]], frame: &[u8]) {
    assert_eq!(buf.len(), cam.get_pixels());
    assert_eq!(frame.len(), cam.get_frame_size());
    assert_eq!(frame.len(), 2*buf.len());

    for (i, v) in buf.iter_mut().enumerate() {
        let (y, cb, cr) = if i % 2 == 0 {
            (frame[2*i] as f32, frame[2*i+1] as f32, frame[2*i+3] as f32)
        } else {
            (frame[2*i] as f32, frame[2*i-1] as f32, frame[2*i+1] as f32)
        };

        let mut r = y + 1.402*(cr - 128.);
        let mut g = y - 0.344136*(cb - 128.) - 0.714136*(cr - 128.);
        let mut b = y + 1.772*(cb - 128.);
        if r > 255. { r = 255.; }
        if r < 0. { r = 0.; }
        if g > 255. { g = 255.; }
        if g < 0. { g = 0.; }
        if b > 255. { b = 255.; }
        if b < 0. { b = 0.; }

        *v = [r as u8, g as u8, b as u8];
    }
}

fn demosaic_grey(cam: &Cam, buf: &mut [[u8; 3]], frame: &[u8]) {
    assert_eq!(buf.len(), cam.get_pixels());
    assert_eq!(frame.len(), cam.get_frame_size());
    assert_eq!(frame.len(), buf.len());

    buf.iter_mut().zip(frame).for_each(|(a, b)|
        *a = if *b > 250 {
            [255, 0, 0]
        } else if *b > 200 {
            [0, 255, 0]
        } else if *b > 128 {
            [0, 0, 255]
        } else {
            [0, 0, 0]
        }
    );
}

pub fn demosaic(cam: &Cam, buf: &mut [[u8; 3]], frame: &[u8]) {
    match &cam.get_format() {
        b"YUYV" => demosaic_yuyv(&cam, buf, frame),
        b"RGGB" => demosaic_rggb(&cam, buf, frame),
        b"GREY" => demosaic_grey(&cam, buf, frame),
        _ => unreachable!(),
    };
}