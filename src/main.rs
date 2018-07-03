#![feature(exact_chunks)]
extern crate rscam;
extern crate winit;

#[macro_use]
extern crate vulkano;
#[macro_use]
extern crate vulkano_shader_derive;
extern crate vulkano_win;

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

use winit::dpi::LogicalSize;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{thread, env, time, str};

const DEFAULT_DIMENSIONS: [u32; 2] = [2448/4, 2048/4];

mod cam;
mod demosaic;
mod rggb;

mod vs;
mod fs;
mod vs2;
mod fs2;
mod fs2_5;

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

fn main() {
    let device = env::args().nth(1).expect("Provide camera device");

    eprintln!("Waiting for camera... ");
    let cam = loop {
        match cam::Cam::new(&device) {
            Ok(cam) => break cam,
            Err(e) => {
                println!("{:?}", e);
                thread::sleep(time::Duration::from_secs(1))
            },
        }
    };

    let resolution = cam.get_resolution();

    let mut dimensions = DEFAULT_DIMENSIONS;

    let extensions = vulkano_win::required_extensions();
    let instance = vulkano::instance::Instance::new(None, &extensions, &[])
        .expect("failed to create instance");

    let physical = vulkano::instance::PhysicalDevice::enumerate(&instance)
                            .next().expect("no device available");
    eprintln!("Using device: {} (type: {:?})", physical.name(), physical.ty());

    let mut events_loop = winit::EventsLoop::new();
    let surface = winit::WindowBuilder::new()
        .with_title("Camera")
        .with_dimensions((dimensions[0], dimensions[1]).into())
        .with_min_dimensions((dimensions[0], dimensions[1]).into())
        .with_decorations(true)
        //.with_fullscreen(Some(events_loop.get_primary_monitor()))
        .build_vk_surface(&events_loop, instance.clone())
        .expect("failed to build window");

    //surface.window().get_hidpi_factor(); does not work on wayland yet
    let hidpi = 2.0;

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
            vulkano::swapchain::PresentMode::Mailbox, true, None
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

    let cross_vertex_buffer = CpuAccessibleBuffer::<[Vertex]>::from_iter(
        device.clone(), vulkano::buffer::BufferUsage::all(),
       [
           Vertex { position: [-1.0,  0.0 ] },
           Vertex { position: [ 1.0,  0.0 ] },
           Vertex { position: [ 0.0, -1.0 ] },
           Vertex { position: [ 0.0,  1.0 ] },
       ].iter().cloned()
    ).expect("failed to create buffer");

    let mut grid = Vec::with_capacity(6*2*2);

    for c in (1..10).map(|i| i as f32/10.) {
        grid.extend_from_slice(&[
            [-1.,  c ], [ 1., c ],
            [ c , -1.], [ c , 1.],
            [-1., -c ], [ 1.,-c ],
            [-c , -1.], [-c , 1.],
        ]);
    }

    let grid_vertex_buffer = CpuAccessibleBuffer::<[Vertex]>::from_iter(
        device.clone(), vulkano::buffer::BufferUsage::all(),
        grid.iter().map(|v| Vertex { position: [v[0], v[1]] } )
    ).expect("failed to create buffer");


    let vs = vs::Shader::load(device.clone())
        .expect("failed to create shader module");
    let fs = fs::Shader::load(device.clone())
        .expect("failed to create shader module");

    let vs2 = vs2::Shader::load(device.clone())
        .expect("vs2: failed to create shader module");
    let fs2 = fs2::Shader::load(device.clone())
        .expect("fs2: failed to create shader module");
    let fs2_5 = fs2_5::Shader::load(device.clone())
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

    let cross_pipeline = Arc::new(vulkano::pipeline::GraphicsPipeline::start()
        .vertex_input_single_buffer::<Vertex>()
        .vertex_shader(vs2.main_entry_point(), ())
        .line_list()
        .viewports_dynamic_scissors_irrelevant(1)
        .fragment_shader(fs2.main_entry_point(), ())
        .render_pass(Subpass::from(renderpass.clone(), 0).unwrap())
        .build(device.clone())
        .expect("Failed to build cross pipeline")
    );

    let grid_pipeline = Arc::new(vulkano::pipeline::GraphicsPipeline::start()
        .vertex_input_single_buffer::<Vertex>()
        .vertex_shader(vs2.main_entry_point(), ())
        .line_list()
        .viewports_dynamic_scissors_irrelevant(1)
        .fragment_shader(fs2_5.main_entry_point(), ())
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
        aspect: [1.0, 1.0], zoom: 1.0, offset: [0.5, 0.5],
    };

    let mut lmb_pressed = false;
    let mut old_coor: Option<[f32; 2]> = None;
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
            let flag = guard.ts != frame_ts;
            if flag {
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
                dyn_state.clone(),
                vertex_buffer.clone(),
                set.clone(), push_consts,
            ).expect("Main pipeline draw fail");

        if grid_on {
            cbb = cbb
                .draw(
                    grid_pipeline.clone(),
                    dyn_state.clone(),
                    grid_vertex_buffer.clone(),
                    (), push_consts,
                ).expect("Cross pipeline draw fail")
                .draw(
                    cross_pipeline.clone(),
                    dyn_state.clone(),
                    cross_vertex_buffer.clone(),
                    (), push_consts,
                ).expect("Cross pipeline draw fail");
        }

        let cb = cbb.end_render_pass().unwrap().build().unwrap();

        let future = previous_frame.join(future)
            .then_execute(queue.clone(), cb).expect("fail2")
            .then_swapchain_present(queue.clone(), swapchain.clone(), image_num)
            .then_signal_fence_and_flush().expect("fail1");
        previous_frame = Box::new(future) as Box<_>;

        let mut done = false;
        events_loop.poll_events(|ev| {
            use winit::Event::WindowEvent;
            use winit::WindowEvent::*;

            match ev {
                WindowEvent { event: CloseRequested, .. } => done = true,
                WindowEvent {
                    event: KeyboardInput {
                        input: winit::KeyboardInput {
                            state: winit::ElementState::Pressed,
                            virtual_keycode: Some(keycode),
                            ..
                        }, ..
                    }, ..
                } => {
                    use winit::VirtualKeyCode::*;
                    match keycode {
                        Escape => done = true,
                        Space => { pause.fetch_nand(true, Ordering::Relaxed); },
                        G => grid_on = !grid_on,
                        R => {
                            push_consts.zoom = 1.0;
                            push_consts.offset = [0.5, 0.5];
                        },
                        _ => (),
                    }
                },

                winit::Event::WindowEvent {
                    event: winit::WindowEvent::Resized (LogicalSize { width, height } ), ..
                } => {
                    recreate_swapchain = true;
                    dimensions = [(hidpi*width) as u32, (hidpi*height) as u32];
                },
                winit::Event::WindowEvent {
                    event: winit::WindowEvent::MouseWheel {
                        delta,
                        phase: winit::TouchPhase::Moved,
                        ..
                    }, ..
                } => {
                    match delta {
                        winit::MouseScrollDelta::LineDelta(_, d) => {
                            if d > 0. {
                                push_consts.zoom *= 1.5;
                            } else if d < 0.{
                                push_consts.zoom /= 1.5;
                            }
                        },
                        winit::MouseScrollDelta::PixelDelta(
                            winit::dpi::LogicalPosition { y, .. }
                        ) => {
                            push_consts.zoom *= 1.03f32.powf(y as f32)
                        },
                    }
                },
                winit::Event::WindowEvent {
                    event: winit::WindowEvent::CursorMoved {
                        position: winit::dpi::LogicalPosition { x, y },
                        ..
                    },
                    ..
                } => {
                    let x = hidpi*x;
                    let y = hidpi*y;
                    if lmb_pressed {
                        if let Some(xy) = old_coor {
                            let z = push_consts.zoom;
                            let k_x =
                                -push_consts.aspect[0]*(dimensions[0] as f32)*z;
                            let k_y =
                                -push_consts.aspect[1]*(dimensions[1] as f32)*z;
                            push_consts.offset[0] = old_offset[0]
                                + (x as f32 - xy[0])/k_x;
                            push_consts.offset[1] = old_offset[1]
                                + (y as f32 - xy[1])/k_y;
                        } else {
                            old_coor = Some([x as f32, y as f32]);
                            old_offset = push_consts.offset;
                        }
                    }
                },
                winit::Event::WindowEvent {
                    event: winit::WindowEvent::MouseInput {
                        state,
                        button: winit::MouseButton::Left,
                        ..
                    }, ..
                } => {
                    lmb_pressed = state == winit::ElementState::Pressed;
                    if !lmb_pressed {
                        old_coor = None;
                    }
                },
                _ => (), //println!("{:?}", ev),
            }
        });
        if done { return; }
    }
}
