extern crate rscam;
extern crate winit;
extern crate png;

#[macro_use] extern crate vulkano;
extern crate vulkano_shaders;
extern crate vulkano_win;
extern crate structopt;
#[macro_use] extern crate structopt_derive;

use vulkano_win::VkSurfaceBuild;
use vulkano::sync::GpuFuture;
use vulkano::framebuffer::Subpass;
use vulkano::descriptor::descriptor_set::PersistentDescriptorSet;
use vulkano::framebuffer::Framebuffer;
use vulkano::buffer::{CpuAccessibleBuffer, CpuBufferPool};
use vulkano::image::Dimensions;
use vulkano::image::StorageImage;
use vulkano::command_buffer::AutoCommandBufferBuilder;
use vulkano::swapchain::acquire_next_image;
use vulkano::swapchain::Swapchain;
use vulkano::swapchain::SwapchainCreationError;
use vulkano::pipeline::viewport::Viewport;
use vulkano::device::Device;
use vulkano::command_buffer::DynamicState;

use png::HasParameters;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{str, slice};
use std::fs::File;
use std::io::BufWriter;

use structopt::StructOpt;

const DEFAULT_DIMENSIONS: [u32; 2] = [2448/4, 2048/4];

mod cam;
mod demosaic;
mod rggb;
mod cli;

mod shaders;

#[derive(Debug, Clone)]
struct Vertex { position: [f32; 2] }
impl_vertex!(Vertex, position);

#[repr(C)]
#[derive(Copy, Clone)]
struct PushConstant {
    aspect: [f32; 2],
    offset: [f32; 2],
    zoom: f32,
}

fn rgb2rgba(pix: &[u8; 3]) -> [u8; 4] {
    [pix[0], pix[1], pix[2], 255]
}

fn main() -> Result<(), Box<std::error::Error>> {
    let args = cli::Cli::from_args();

    eprintln!("Waiting for camera... ");
    let cam = cam::Cam::new(&args.camera)?;

    let resolution = cam.get_resolution();

    let mut dimensions = DEFAULT_DIMENSIONS;

    let extensions = vulkano_win::required_extensions();
    let instance = vulkano::instance::Instance::new(None, &extensions, None)
        .expect("failed to create instance");

    let physical = vulkano::instance::PhysicalDevice::enumerate(&instance)
                            .next().expect("no device available");
    eprintln!("Using device: {} (type: {:?})", physical.name(), physical.ty());

    let mut events_loop = winit::EventsLoop::new();
    let surface = winit::WindowBuilder::new()
        .with_title("Camera")
        .with_dimensions((dimensions[0], dimensions[1]).into())
        //.with_min_dimensions((dimensions[0], dimensions[1]).into())
        .with_decorations(true)
        //.with_fullscreen(Some(events_loop.get_primary_monitor()))
        .build_vk_surface(&events_loop, instance.clone())
        .expect("failed to build window");

    let mut hidpi = surface.window().get_hidpi_factor();

    let queue = physical.queue_families().find(|&q|
        q.supports_graphics() && surface.is_supported(q).unwrap_or(false)
    ).expect("couldn't find a graphical queue family");
    let device_ext = vulkano::device::DeviceExtensions {
        khr_swapchain: true,
        .. vulkano::device::DeviceExtensions::none()
    };
    let (device, mut queues) = Device::new(
        physical, physical.supported_features(), &device_ext,
        [(queue, 0.5)].iter().cloned()
    ).expect("failed to create device");
    let queue = queues.next().unwrap();


    let pause = Arc::new(AtomicBool::new(false));
    let is_grey = &cam.get_format() == b"GREY";
    let cam_mutex = cam.run_worker(pause.clone());




    let (mut swapchain, mut images) = {
        let caps = surface.capabilities(physical)
            .expect("failed to get surface capabilities");

        dimensions = caps.current_extent.unwrap_or(dimensions);
        let usage = caps.supported_usage_flags;
        let alpha = caps.supported_composite_alpha.iter().next().unwrap();
        let format = caps.supported_formats[0].0;

        Swapchain::new(
            device.clone(), surface.clone(), caps.min_image_count,
            format, dimensions, 1, usage, &queue,
            vulkano::swapchain::SurfaceTransform::Identity, alpha,
            args.mode, true, None
        ).expect("failed to create swapchain")
    };

    let vertex_buffer = CpuAccessibleBuffer::<[Vertex]>::from_iter(
        device.clone(), vulkano::buffer::BufferUsage::all(),
       [
            Vertex { position: [-1.0, -1.0 ] },
            Vertex { position: [-1.0,  1.0 ] },
            Vertex { position: [ 1.0, -1.0 ] },
            Vertex { position: [ 1.0,  1.0 ] },
       ].iter().cloned()
    ).expect("failed to create buffer");

    let mut grid = Vec::new();

    let w_f32 = (resolution.0 as f32)/2.;
    for i in (args.grid_step..resolution.0).step_by(args.grid_step as usize) {
        let c = (i as f32)/w_f32 - 1.;
        grid.extend_from_slice(&[[c, -1.], [c, 1.]]);
    }

    let h_f32 = (resolution.1 as f32)/2.;
    for i in (args.grid_step..resolution.1).step_by(args.grid_step as usize) {
        let c = (i as f32)/h_f32 - 1.;
        grid.extend_from_slice(&[[-1., c], [1., c]]);
    }

    let grid_vertex_buffer = CpuAccessibleBuffer::<[Vertex]>::from_iter(
        device.clone(), vulkano::buffer::BufferUsage::all(),
        grid.iter().map(|v| Vertex { position: [v[0], v[1]] } )
    ).expect("failed to create buffer");


    let vs = shaders::vs::Shader::load(device.clone())
        .expect("failed to create shader module");
    let fs = shaders::fs::Shader::load(device.clone())
        .expect("failed to create shader module");

    let vs2 = shaders::vs2::Shader::load(device.clone())
        .expect("vs2: failed to create shader module");
    let fs2 = shaders::fs2::Shader::load(device.clone())
        .expect("fs2: failed to create shader module");

    let renderpass = Arc::new(
        single_pass_renderpass!(device.clone(),
            attachments: {
                color: {
                    load: Clear,
                    store: Store,
                    format: swapchain.format(),
                    samples: 1,
                }
            },
            pass: {
                color: [color],
                depth_stencil: {}
            }
        ).unwrap()
    );

    let texture = StorageImage::new(
        device.clone(),
        Dimensions::Dim2d { width: resolution.0, height: resolution.1 },
        vulkano::format::R8G8B8A8Unorm,
        Some(queue.family()),
    ).unwrap();


    let addr_mode = vulkano::sampler::SamplerAddressMode::ClampToBorder(
        vulkano::sampler::BorderColor::IntTransparentBlack
    );
    let sampler = vulkano::sampler::Sampler::new(
        device.clone(),
        vulkano::sampler::Filter::Nearest,
        vulkano::sampler::Filter::Linear,
        vulkano::sampler::MipmapMode::Nearest,
        addr_mode, addr_mode, addr_mode,
        0.0, 1.0, 0.0, 0.0
    ).unwrap();

    let pipeline = Arc::new(vulkano::pipeline::GraphicsPipeline::start()
        .vertex_input_single_buffer::<Vertex>()
        .vertex_shader(vs.main_entry_point(), ())
        .triangle_strip()
        .viewports_dynamic_scissors_irrelevant(1)
        .fragment_shader(fs.main_entry_point(), ())
        .blend_alpha_blending()
        .render_pass(Subpass::from(renderpass.clone(), 0).unwrap())
        .build(device.clone())
        .expect("Failed to build main pipeline")
    );

    let grid_pipeline = Arc::new(vulkano::pipeline::GraphicsPipeline::start()
        .vertex_input_single_buffer::<Vertex>()
        .vertex_shader(vs2.main_entry_point(), ())
        .line_list()
        .viewports_dynamic_scissors_irrelevant(1)
        .fragment_shader(fs2.main_entry_point(), ())
        .render_pass(Subpass::from(renderpass.clone(), 0).unwrap())
        .build(device.clone())
        .expect("Failed to build grid pipeline")
    );

    let set = Arc::new(PersistentDescriptorSet::start(pipeline.clone(), 0)
        .add_sampled_image(texture.clone(), sampler.clone()).unwrap()
        .build().unwrap()
    );

    let mut framebuffers: Vec<Arc<Framebuffer<_,_>>> = images.iter()
        .map(|image|
            Arc::new(Framebuffer::start(renderpass.clone())
                 .add(image.clone()).unwrap()
                 .build().unwrap())
        ).collect::<Vec<Arc<Framebuffer<_,_>>>>();

    let mut recreate_swapchain = false;

    let prev_frame = Box::new(vulkano::sync::now(device.clone()));
    let mut previous_frame = prev_frame as Box<GpuFuture>;

    let mut push_consts = PushConstant{
        aspect: [1.0, 1.0], zoom: 1.0, offset: [0., 0.],
    };

    let mut lmb_pressed = false;
    let mut init_coor = [0f32; 2];
    let mut mouse_coor = [0f32; 2];
    let mut old_offset = push_consts.offset;
    let mut grid_on = false;

    let mut frame_ts = 0u64;

    let buf_pool = CpuBufferPool::upload(device.clone());
    let mut chunk = buf_pool.chunk(
        (0..resolution.0*resolution.1).map(|_| [0u8, 0, 0, 255]))
        .unwrap();

    loop {
        previous_frame.cleanup_finished();

        if recreate_swapchain {
            dimensions = surface.capabilities(physical)
                .expect("failed to get surface capabilities")
                .current_extent.unwrap_or(dimensions);

            match swapchain.recreate_with_dimension(dimensions) {
                Ok((new_swapchain, new_images)) => {
                    swapchain = new_swapchain;
                    images = new_images;
                },
                Err(SwapchainCreationError::UnsupportedDimensions) => {
                    continue;
                },
                Err(err) => panic!("{:?}", err)
            };

            let r_p1 = (dimensions[0] as f32)/(dimensions[1] as f32);
            let r_p2 = (resolution.0 as f32)/(resolution.1 as f32);

            push_consts.aspect = if r_p1 > r_p2 {
                [r_p2/r_p1, 1.]
            } else {
                [1.0, r_p1/r_p2]
            };

            framebuffers = images.iter().map(|image|
                Arc::new(Framebuffer::start(renderpass.clone())
                         .add(image.clone()).unwrap()
                         .build().unwrap())
                ).collect::<Vec<_>>();

            recreate_swapchain = false;
        }

        let next_img = acquire_next_image(swapchain.clone(), None);
        let (image_num, future) = match next_img {
            Ok(r) => r,
            Err(vulkano::swapchain::AcquireError::OutOfDate) => {
                recreate_swapchain = true;
                continue;
            },
            Err(err) => panic!("{:?}", err)
        };

        let dyn_state = DynamicState {
            line_width: None,
            viewports: Some(vec![Viewport {
                origin: [0.0, 0.0],
                dimensions: [dimensions[0] as f32, dimensions[1] as f32],
                depth_range: 0.0 .. 1.0,
            }]),
            scissors: None,
        };

        {
            let guard = cam_mutex.lock().unwrap();
            if guard.ts != frame_ts {
                frame_ts = guard.ts;
                match buf_pool.chunk(guard.buf.iter().map(rgb2rgba)) {
                    Ok(c) => chunk = c,
                    Err(e) => eprintln!("BufPool error: {:?}", e),
                }
            }
        };

        let mut cbb = AutoCommandBufferBuilder
            ::primary_one_time_submit(device.clone(), queue.family())
            .unwrap()
            .copy_buffer_to_image(chunk.clone(), texture.clone())
            .expect("Failed to copy data to texture")
            .begin_render_pass(
                framebuffers[image_num].clone(), false,
                vec![[0.0, 0.0, 0.0, 1.0].into()]).unwrap()
            .draw(
                pipeline.clone(),
                &dyn_state,
                vertex_buffer.clone(),
                set.clone(), push_consts,
            ).expect("Main pipeline draw fail");

        if grid_on {
            cbb = cbb
                .draw(
                    grid_pipeline.clone(),
                    &dyn_state,
                    grid_vertex_buffer.clone(),
                    (), push_consts,
                ).expect("grid pipeline draw fail");
        }

        let cb = cbb.end_render_pass().unwrap().build().unwrap();

        let future = previous_frame.join(future)
            .then_execute(queue.clone(), cb).unwrap()
            .then_swapchain_present(queue.clone(), swapchain.clone(), image_num)
            .then_signal_fence_and_flush().unwrap();
        previous_frame = Box::new(future) as Box<vulkano::sync::GpuFuture>;

        let mut done = false;
        events_loop.poll_events(|event| {
            if let winit::Event::WindowEvent{ event, .. } = event {
                use winit::WindowEvent::*;

                match event {
                    CloseRequested => done = true,
                    HiDpiFactorChanged(val) => hidpi = val,
                    Resized(size) => {
                        recreate_swapchain = true;
                        dimensions = [
                            (hidpi*size.width) as u32,
                            (hidpi*size.height) as u32,
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
                            Escape => done = true,
                            Space => {
                                pause.fetch_nand(true, Ordering::Relaxed);
                            },
                            G => grid_on = !grid_on,
                            R => {
                                push_consts.zoom = 1.0;
                                push_consts.offset = [0., 0.];
                            },
                            S => {
                                let guard = cam_mutex.lock().unwrap();

                                let path = format!("{}.png", guard.ts);
                                let file = File::create(&path).unwrap();

                                let mut bw = BufWriter::new(file);
                                let mut encoder = png::Encoder::new(
                                    &mut bw, resolution.0, resolution.1,
                                );
                                encoder.set(png::BitDepth::Eight);
                                if is_grey {
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
                                push_consts.zoom * 1.5
                            } else {
                                push_consts.zoom / 1.5
                            },
                            PixelDelta(
                                winit::dpi::LogicalPosition { y, .. }
                            ) => {
                                push_consts.zoom * 1.03f32.powf(y as f32)
                            },
                        };

                        let xg = 2.*mouse_coor[0]/(dimensions[0] as f32) - 1.;
                        let yg = 2.*mouse_coor[1]/(dimensions[1] as f32) - 1.;

                        let k = (1./new_zoom - 1./push_consts.zoom)/2.;

                        push_consts.offset[0] += k*xg/push_consts.aspect[0];
                        push_consts.offset[1] += k*yg/push_consts.aspect[1];
                        push_consts.zoom = new_zoom;
                    },
                    CursorMoved {
                        position: winit::dpi::LogicalPosition { x, y },
                        ..
                    } => {
                        let x = (hidpi*x) as f32;
                        let y = (hidpi*y) as f32;
                        mouse_coor = [x, y];
                        if lmb_pressed {
                            let z = push_consts.zoom;
                            let dim = dimensions;
                            let ic = init_coor;
                            let k_x = push_consts.aspect[0]*(dim[0] as f32)*z;
                            let k_y = push_consts.aspect[1]*(dim[1] as f32)*z;
                            push_consts.offset = [
                                old_offset[0] + (x - ic[0])/k_x,
                                old_offset[1] + (y - ic[1])/k_y,
                            ];
                        }
                    },
                    MouseInput {
                        state,
                        button: winit::MouseButton::Left,
                        ..
                    } => {
                        lmb_pressed = state == winit::ElementState::Pressed;
                        if lmb_pressed {
                            old_offset = push_consts.offset;
                            init_coor = mouse_coor;
                        }
                    },
                    _ => (), //println!("{:?}", ev),
                }
            }
        });
        if done { return Ok(()); }
    }
}
