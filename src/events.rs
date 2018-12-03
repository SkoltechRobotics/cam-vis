use super::EngineState;
use winit::Event;
use winit::WindowEvent::*;
use winit;

use png::HasParameters;

use std::sync::atomic::Ordering;
use std::slice;
use std::fs::File;
use std::io::BufWriter;

pub(crate) fn handle(event: Event, state: &mut EngineState) {
    if let winit::Event::WindowEvent{ event, .. } = event {
        match event {
            CloseRequested => state.done = true,
            HiDpiFactorChanged(val) => state.hidpi = val,
            Resized(size) => {
                state.recreate_swapchain = true;
                state.dimensions = [
                    (state.hidpi*size.width) as u32,
                    (state.hidpi*size.height) as u32,
                ];
            },
            KeyboardInput {
                input: winit::KeyboardInput {
                    state: winit::ElementState::Pressed,
                    virtual_keycode: Some(keycode),
                    ..
                }, ..
            } => {
                use winit::VirtualKeyCode::*;
                match keycode {
                    Escape => state.done = true,
                    Space => {
                        state.pause.fetch_nand(true, Ordering::Relaxed);
                    },
                    G => state.grid_on = !state.grid_on,
                    R => {
                        state.push_consts.zoom = 1.0;
                        state.push_consts.offset = [0., 0.];
                    },
                    S => {
                        let guard = state.cam_mutex.lock().unwrap();

                        let path = format!("{}.png", guard.ts);
                        let file = File::create(&path).unwrap();

                        let mut bw = BufWriter::new(file);
                        let mut encoder = png::Encoder::new(
                            &mut bw, state.resolution[0], state.resolution[1],
                        );
                        encoder.set(png::BitDepth::Eight);
                        if state.is_grey {
                            encoder.set(png::ColorType::Grayscale);
                            let mut w = encoder.write_header().unwrap();
                            let data: Vec<u8> = guard.buf
                                .iter()
                                .map(|p| p[0])
                                .collect();
                            w.write_image_data(&data).unwrap();
                        } else {
                            encoder.set(png::ColorType::RGB);
                            let mut w = encoder.write_header().unwrap();
                            let buf = guard.buf.as_slice();
                            let data = unsafe {
                                slice::from_raw_parts(
                                    buf.as_ptr() as *const u8,
                                    3*buf.len(),
                                )
                            };
                            w.write_image_data(&data).unwrap();
                        }
                        println!("Saved: {}", path);
                    }
                    _ => (),
                }
            },
            winit::WindowEvent::MouseWheel {
                delta,
                phase: winit::TouchPhase::Moved,
                ..
            } => {
                use winit::MouseScrollDelta::*;

                let new_zoom = match delta {
                    LineDelta(_, d) => if d > 0. {
                        state.push_consts.zoom * 1.5
                    } else {
                        state.push_consts.zoom / 1.5
                    },
                    PixelDelta(
                        winit::dpi::LogicalPosition { y, .. }
                    ) => {
                        state.push_consts.zoom * 1.03f32.powf(y as f32)
                    },
                };

                let dims = state.dimensions;
                let push_consts = &mut state.push_consts;
                let xg = 2.*state.mouse_coor[0]/(dims[0] as f32) - 1.;
                let yg = 2.*state.mouse_coor[1]/(dims[1] as f32) - 1.;

                let k = (1./new_zoom - 1./push_consts.zoom)/2.;

                push_consts.offset[0] += k*xg/push_consts.aspect[0];
                push_consts.offset[1] += k*yg/push_consts.aspect[1];
                push_consts.zoom = new_zoom;
            },
            CursorMoved {
                position: winit::dpi::LogicalPosition { x, y },
                ..
            } => {
                let x = (state.hidpi*x) as f32;
                let y = (state.hidpi*y) as f32;
                state.mouse_coor = [x, y];
                if state.lmb_pressed {
                    let z = state.push_consts.zoom;
                    let dim = state.dimensions;
                    let ic = state.init_coor;
                    let k_x = state.push_consts.aspect[0]*(dim[0] as f32)*z;
                    let k_y = state.push_consts.aspect[1]*(dim[1] as f32)*z;
                    state.push_consts.offset = [
                        state.old_offset[0] + (x - ic[0])/k_x,
                        state.old_offset[1] + (y - ic[1])/k_y,
                    ];
                }
            },
            MouseInput {
                state: mouse_state,
                button: winit::MouseButton::Left,
                ..
            } => {
                state.lmb_pressed = mouse_state == winit::ElementState::Pressed;
                if state.lmb_pressed {
                    state.old_offset = state.push_consts.offset;
                    state.init_coor = state.mouse_coor;
                }
            },
            _ => (), //println!("{:?}", ev),
        }
    }
}