use vulkano_win::VkSurfaceBuild;
use vulkano::buffer::{BufferUsage, CpuAccessibleBuffer};
use vulkano::instance::{ Instance, PhysicalDevice };
use vulkano::device::{ Device, DeviceExtensions };
use vulkano::descriptor::descriptor_set::PersistentDescriptorSet;
use vulkano::format::Format;
use vulkano::image::{ Dimensions, ImageUsage, ImmutableImage, SwapchainImage };
use vulkano::sampler::{ Sampler, Filter, MipmapMode, SamplerAddressMode };
use vulkano::swapchain;
use vulkano::swapchain::{ AcquireError, Swapchain, SurfaceTransform, CompositeAlpha, PresentMode, FullscreenExclusive, ColorSpace, SwapchainCreationError };
use vulkano::pipeline::GraphicsPipeline;
use vulkano::pipeline::viewport::Viewport;
use vulkano::framebuffer::{ Framebuffer, FramebufferAbstract, RenderPassAbstract, Subpass };
use vulkano::command_buffer::{ AutoCommandBufferBuilder, DynamicState };
use vulkano::sync;
use vulkano::sync::{ FlushError, GpuFuture };
use winit::window::{ Window, WindowBuilder };
use winit::event_loop::{ EventLoop, ControlFlow };
use winit::event::{ Event, WindowEvent };
use std::sync::Arc;
use png;
use std::io::Cursor;

fn main() {
    let instance = {
        let extensions = vulkano_win::required_extensions();
        Instance::new(None, &extensions, None).expect("Failed to create Vulkano instance")
    };

    println!("Available physical devices:");
    for device in PhysicalDevice::enumerate(&instance) {
        println!("{}\t{:?}", device.name(), device.ty());
    }
    println!("");

    let physical = PhysicalDevice::enumerate(&instance).next().unwrap();
    println!("Using device: {} (type: {:?})", physical.name(), physical.ty());

    let event_loop = EventLoop::new();
    let surface = WindowBuilder::new().build_vk_surface(&event_loop, instance.clone()).unwrap();

    let queue_family = physical.queue_families().find(|&q| {
        q.supports_graphics() && surface.is_supported(q).unwrap_or(false)
    }).unwrap();

    let device_ext = DeviceExtensions {
        khr_swapchain: true,
        ..DeviceExtensions::none()
    };
    let (device, mut queues) = Device::new(
        physical,
        physical.supported_features(),
        &device_ext,
        [(queue_family, 0.5)].iter().cloned()
    ).unwrap();

    let queue = queues.next().unwrap();

    let (mut swapchain, images) = {
        let capabilities = surface.capabilities(physical).unwrap();

        println!("Supported formats:");
        for f in &capabilities.supported_formats {
            println!("{:?}", f);
        }
        println!("");

        let format = Format::B8G8R8A8Srgb;
        if !capabilities.supported_formats.iter().any(|(f, _)| f == &format) {
            panic!("Unsupported swapchain format {:?}", format);
        }

        let dimensions: [u32; 2] = surface.window().inner_size().into();

        Swapchain::new(
            device.clone(),
            surface.clone(),
            capabilities.min_image_count,
            format,
            dimensions,
            1,
            ImageUsage::color_attachment(),
            &queue,
            SurfaceTransform::Identity,
            CompositeAlpha::Opaque,
            PresentMode::Fifo,
            FullscreenExclusive::Default,
            true,
            ColorSpace::SrgbNonLinear
        ).unwrap()
    };

    let vertex_buffer = {
        #[derive(Default, Debug, Clone)]
        struct Vertex {
            position: [f32; 2],
        }
        vulkano::impl_vertex!(Vertex, position);

        CpuAccessibleBuffer::from_iter(
            device.clone(),
            BufferUsage::vertex_buffer(),
            false,
            [
                Vertex { position: [ -1.0, 1.0 ] },
                Vertex { position: [ 1.0, -1.0 ] },
                Vertex { position: [ -1.0, -1.0 ] },
                Vertex { position: [ -1.0, 1.0 ] },
                Vertex { position: [ 1.0, -1.0 ] },
                Vertex { position: [ 1.0, 1.0 ] },
            ]
            .iter()
            .cloned()
        ).unwrap()
    };

    mod vs {
        vulkano_shaders::shader! {
            ty: "vertex",
            src: "
                #version 450

                layout(location = 0) in vec2 position;

                layout(push_constant) uniform PushConstants {
                    float win_ratio;
                    float tex_ratio;
                } pc;

                layout(location = 0) out vec2 tex_coords;

                void main() {
                    gl_Position = vec4(position, 0.0, 1.0);

                    vec2 tex_0_to_1 = (position + vec2(1.0)) / vec2(2.0);
                    if (pc.win_ratio > pc.tex_ratio) {
                        float ratio = (1 / pc.tex_ratio) * pc.win_ratio;
                        float correction = (ratio - 1) / (2 * ratio);
                        tex_coords.x = ((tex_0_to_1 - correction) * ratio).x;
                        tex_coords.y = tex_0_to_1.y;
                    } else {
                        float ratio = pc.tex_ratio * (1 / pc.win_ratio);
                        float correction = (ratio - 1) / (2 * ratio);
                        tex_coords.y = ((tex_0_to_1 - correction) * ratio).y;
                        tex_coords.x = tex_0_to_1.x;
                    }
                }
            "
        }
    }

    mod fs {
        vulkano_shaders::shader! {
            ty: "fragment",
            src: "
                #version 450

                layout(location = 0) in vec2 tex_coords;
                layout(location = 0) out vec4 f_color;

                layout(set = 0, binding = 0) uniform sampler2D tex;

                void main() {
                    f_color = texture(tex, tex_coords);
                }
            "
        }
    }

    let vs = vs::Shader::load(device.clone()).unwrap();
    let fs = fs::Shader::load(device.clone()).unwrap();

    let render_pass = Arc::new(
        vulkano::single_pass_renderpass!(
            device.clone(),
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

    let mut tex_ratio;
    let (texture, tex_future) = {
        let png_bytes = include_bytes!("test_image.png").to_vec();
        let cursor = Cursor::new(png_bytes);
        let decoder = png::Decoder::new(cursor);
        let (info, mut reader) = decoder.read_info().unwrap();
        let dimensions = Dimensions::Dim2d {
            width: info.width,
            height: info.height,
        };
        tex_ratio = (info.width as f32) / (info.height as f32);
        let mut image_data = Vec::new();
        image_data.resize((info.width * info.height * 4) as usize, 0);
        reader.next_frame(&mut image_data).unwrap();

        ImmutableImage::from_iter(
            image_data.iter().cloned(),
            dimensions,
            Format::R8G8B8A8Srgb,
            queue.clone(),
        ).unwrap()
    };

    let sampler = Sampler::new(
        device.clone(),
        Filter::Linear,
        Filter::Linear,
        MipmapMode::Nearest,
        SamplerAddressMode::ClampToEdge,
        SamplerAddressMode::ClampToEdge,
        SamplerAddressMode::ClampToEdge,
        0.0,
        1.0,
        0.0,
        0.0
    ).unwrap();

    let pipeline = Arc::new(
        GraphicsPipeline::start()
            .vertex_input_single_buffer()
            .vertex_shader(vs.main_entry_point(), ())
            .triangle_list()
            .viewports_dynamic_scissors_irrelevant(1)
            .fragment_shader(fs.main_entry_point(), ())
            .render_pass(Subpass::from(render_pass.clone(), 0).unwrap())
            .build(device.clone())
            .unwrap()
    );

    let mut dynamic_state = DynamicState {
        line_width: None,
        viewports: None,
        scissors: None,
        compare_mask: None,
        write_mask: None,
        reference: None,
    };

    let layout = pipeline.layout().descriptor_set_layout(0).unwrap();
    let set = Arc::new(
        PersistentDescriptorSet::start(layout.clone())
            .add_sampled_image(texture.clone(), sampler.clone())
            .unwrap()
            .build()
            .unwrap(),
    );

    let mut framebuffers = window_size_dependent_setup(&images, render_pass.clone(), &mut dynamic_state);

    let mut recreate_swapchain = false;
    let mut previous_frame_end = Some(tex_future.boxed());
    
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::WindowEvent {
                event:  WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            },
            Event::WindowEvent {
                event:  WindowEvent::Resized(_),
                ..
            } => {
                recreate_swapchain = true;
            },
            Event::RedrawEventsCleared => {
                previous_frame_end.as_mut().unwrap().cleanup_finished();

                if recreate_swapchain {
                    let dimensions: [u32; 2] = surface.window().inner_size().into();
                    let (new_swapchain, new_images) =
                        match swapchain.recreate_with_dimensions(dimensions) {
                            Ok(r) => r,
                            Err(SwapchainCreationError::UnsupportedDimensions) => return,
                            Err(e) => panic!("Failed to recreate swapchain: {:?}", e),
                        };
                    
                    swapchain = new_swapchain;
                    framebuffers = window_size_dependent_setup(&new_images, render_pass.clone(), &mut dynamic_state);
                    recreate_swapchain = false;
                }

                let (image_num, suboptimal, acquire_future) =
                    match swapchain::acquire_next_image(swapchain.clone(), None) {
                        Ok(r) => r,
                        Err(AcquireError::OutOfDate) => {
                            recreate_swapchain = true;
                            return;
                        }
                        Err(e) => panic!("Failed to acquire next image: {:?}", e),
                    };
                
                if suboptimal {
                    recreate_swapchain = true;
                }

                let clear_values = vec![[0.0, 0.0, 1.0, ].into()];

                let dimensions: [u32; 2] = surface.window().inner_size().into();
                let push_constants = vs::ty::PushConstants {
                    win_ratio: (dimensions[0] as f32) / (dimensions[1] as f32),
                    tex_ratio
                };

                let mut builder = AutoCommandBufferBuilder::primary_one_time_submit(
                    device.clone(),
                    queue.family(),
                ).unwrap();
                builder
                    .begin_render_pass(framebuffers[image_num].clone(), false, clear_values)
                    .unwrap()
                    .draw(
                        pipeline.clone(),
                        &dynamic_state,
                        vertex_buffer.clone(),
                        set.clone(),
                        push_constants,
                    )
                    .unwrap()
                    .end_render_pass()
                    .unwrap();
                
                let command_buffer = builder.build().unwrap();

                let future = previous_frame_end
                    .take()
                    .unwrap()
                    .join(acquire_future)
                    .then_execute(queue.clone(), command_buffer)
                    .unwrap()
                    .then_swapchain_present(queue.clone(), swapchain.clone(), image_num)
                    .then_signal_fence_and_flush();
                
                match future {
                    Ok(future) => {
                        previous_frame_end = Some(future.boxed());
                    }
                    Err(FlushError::OutOfDate) => {
                        recreate_swapchain = true;
                        previous_frame_end = Some(sync::now(device.clone()).boxed());
                    }
                    Err(e) => {
                        println!("Failed to flush future: {:?}", e);
                        previous_frame_end = Some(sync::now(device.clone()).boxed());
                    }
                }
            },
            _ => ()
        }
    });
}

fn window_size_dependent_setup(
    images: &[Arc<SwapchainImage<Window>>],
    render_pass: Arc<dyn RenderPassAbstract + Send + Sync>,
    dynamic_state: &mut DynamicState,
) -> Vec<Arc<dyn FramebufferAbstract + Send + Sync>> {
    let dimensions = images[0].dimensions();

    let viewport = Viewport {
        origin: [0.0, 0.0],
        dimensions: [dimensions[0] as f32, dimensions[1] as f32],
        depth_range: 0.0..1.0,
    };
    dynamic_state.viewports = Some(vec![viewport]);

    images
        .iter()
        .map(|image| {
            Arc::new(
                Framebuffer::start(render_pass.clone())
                    .add(image.clone())
                    .unwrap()
                    .build()
                    .unwrap(),
            ) as Arc<dyn FramebufferAbstract + Send + Sync>
        })
        .collect::<Vec<_>>()
}
