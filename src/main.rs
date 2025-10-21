use crossterm::style::Stylize;
use crossterm::{QueueableCommand, cursor, event, terminal};
use itertools::izip;
use pipewire as pw;
use pipewire::registry::GlobalObject;
use pipewire::spa::param::format::{MediaSubtype, MediaType};
use pipewire::spa::param::format_utils;
use pw::{properties::properties, spa};
use rustfft::FftPlanner;
use rustfft::num_complex::{Complex32, ComplexFloat};
use spa::pod::Pod;
use std::cell::{Cell, OnceCell};
use std::f32::consts::TAU;
use std::io::{Write, stdout};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;
use std::{mem, slice, thread};

use crate::buffer::Buffer;

mod buffer;

//const FIRE_STRING: [char; 52] = [
//    ' ', ',', ';', '+', 'l', 't', 'g', 't', 'i', '!', 'l', 'I', '?', '/', '\\', '|', ')', '(', '1',
//    '}', '{', ']', '[', 'r', 'c', 'v', 'z', 'j', 'f', 't', 'J', 'U', 'O', 'Q', 'o', 'c', 'x', 'f',
//    'X', 'h', 'q', 'w', 'W', 'B', '8', '&', '%', '$', '#', '@', '"', ';',
//];

struct UserData {
    format: spa::param::audio::AudioInfoRaw,
    cursor_move: bool,
}
const FILTER_NAME: &str = "audio-capture";

//#[derive(Parser)]
//#[clap(name = FILTER_NAME, about = "Audio stream capture example")]
//struct Opt {
//    #[clap(short, long, help = "The target object id to connect to")]
//    target: String,
//}

const MIN_DB: f32 = -90.0;
const MAX_DB: f32 = 20.0;
//const N_FFT: usize = 128 + 64;

fn min_max_norm(n: f32, min_v: f32, max_v: f32) -> f32 {
    (n.min(max_v).max(min_v) - min_v) / (max_v - min_v)
}

#[allow(unused)]
unsafe fn bytes_to<T>(bytes: &[u8]) -> &[T] {
    let ptr = bytes.as_ptr() as *const T;
    let len = bytes.len() / mem::size_of::<T>();

    unsafe { slice::from_raw_parts(ptr, len) }
}

#[allow(unused)]
unsafe fn bytes_to_mut<T>(bytes: &mut [u8]) -> &mut [T] {
    let ptr = bytes.as_ptr() as *mut T;
    let len = bytes.len() / std::mem::size_of::<T>();

    unsafe { slice::from_raw_parts_mut(ptr, len) }
}

pub fn main() -> Result<(), pw::Error> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;

    let monitor_id = get_node_id(
        &mainloop,
        &core,
        "bluez_output.74_45_CE_66_BE_B7.1".to_string(),
    )
    .unwrap();

    let data = UserData {
        format: Default::default(),
        cursor_move: false,
    };

    let props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        // Stream/Audio/Input why this is correct ???
        *pw::keys::MEDIA_CLASS => "Stream/Audio/Input",
        //*pw::keys::AUDIO_FORMAT => "F32LE",
    };

    let stream = pw::stream::StreamBox::new(&core, FILTER_NAME, props)?;

    terminal::enable_raw_mode().unwrap();
    let mut view_buffer = Buffer::<Vec<f32>>::new();

    let (fft, height) = terminal::size().unwrap();
    view_buffer.resize(fft.into(), height as usize);

    let mut fft_planner = FftPlanner::<f32>::new();
    let mut fft_forward = fft_planner.plan_fft(fft.into(), rustfft::FftDirection::Forward);
    let mut fft_buffer = vec![Complex32::default(); fft.into()];
    let mut fft_scratch = fft_buffer.clone();
    let mut fft_window = hann_window(fft.into()).collect::<Vec<_>>();

    view_buffer.on_update(move |buf, width, height, fft_norm| {
        for (col, fft) in fft_norm.iter().enumerate() {
            for (i, row) in (0..height).rev().enumerate() {
                buf[row * width + col] = if *fft < i as f32 {
                    ' '.on_black()
                } else {
                    ' '.stylize()
                };
            }
        }
    });

    let (event_send, event_rev) = mpsc::channel();

    let mainloop_audio = mainloop.clone();
    let _listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(|_stream, user_data, id, param| {
            // NULL means to clear the format
            let Some(param) = param else {
                return;
            };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }

            let (media_type, media_subtype) = match format_utils::parse_format(param) {
                Ok(v) => v,
                Err(_) => return,
            };

            // only accept raw audio
            if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                return;
            }

            // call a helper function to parse the format for us.
            user_data
                .format
                .parse(param)
                .expect("Failed to parse param changed to AudioInfoRaw");

            //println!(
            //    "capturing rate:{} channels:{}",
            //    user_data.format.rate(),
            //    user_data.format.channels()
            //);
        })
        .process(move |stream, user_data| {
            // handle event
            while let Ok(event) = event_rev.try_recv() {
                match event {
                    event::Event::Key(event::KeyEvent {
                        code: event::KeyCode::Char('c'),
                        modifiers: event::KeyModifiers::CONTROL,
                        ..
                    })
                    | event::Event::Key(event::KeyEvent {
                        code: event::KeyCode::Char('q'),
                        ..
                    }) => {
                        mainloop_audio.quit();
                    }
                    event::Event::Resize(fft, height) => {
                        view_buffer.resize(fft.into(), height.into());

                        fft_forward =
                            fft_planner.plan_fft(fft.into(), rustfft::FftDirection::Forward);
                        fft_buffer = vec![Complex32::default(); fft.into()];
                        fft_scratch = fft_buffer.clone();
                        fft_window = hann_window(fft.into()).collect::<Vec<_>>();
                    }
                    _ => {}
                }
            }

            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }
                let data = &mut datas[0];
                let n_channels = user_data.format.channels();

                if let Some(samples) = data.data() {
                    let samples = unsafe { bytes_to::<f32>(samples) };

                    let samples_mono = samples
                        .chunks(n_channels as usize)
                        .map(|s| s.iter().sum::<f32>() / n_channels as f32);

                    izip!(fft_buffer.iter_mut(), samples_mono, fft_window.iter(),)
                        .for_each(|(fft_s, s, w)| *fft_s = (s * w).into());

                    fft_forward.process_with_scratch(&mut fft_buffer, &mut fft_scratch);

                    let mut stdout = stdout();

                    let fft_norm: Vec<_> = fft_buffer
                        .iter()
                        .map(|n| min_max_norm(amp2db(n.abs()), MIN_DB, MAX_DB) * view_buffer.height() as f32 * 0.5)
                        .collect();

                    view_buffer.update(fft_norm);
                    view_buffer.present(&mut stdout).unwrap();

                    user_data.cursor_move = true;
                }
            }
        })
        .register();

    /* Make one parameter with the supported formats. The SPA_PARAM_EnumFormat
     * id means that this is a format enumeration (of 1 value).
     * We leave the channels and rate empty to accept the native graph
     * rate and channels. */
    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    let obj = pw::spa::pod::Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pw::spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values).unwrap()];

    /* Now connect this stream. We ask that our process function is
     * called in a realtime thread. */
    stream.connect(
        spa::utils::Direction::Input,
        Some(monitor_id),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        //| pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;
    do_roundtrip(&mainloop, &core);

    thread::spawn(move || {
        loop {
            if event::poll(Duration::from_millis(100)).unwrap() {
                event_send.send(event::read().unwrap()).unwrap();
            }
        }
    });

    mainloop.run();
    stdout()
        .queue(terminal::Clear(terminal::ClearType::All))
        .unwrap()
        .queue(cursor::Show)
        .unwrap()
        .flush()
        .unwrap();
    terminal::disable_raw_mode().unwrap();

    Ok(())
}

fn get_node_id(
    mainloop: &pw::main_loop::MainLoopRc,
    core: &pw::core::CoreRc,
    name: String,
) -> Result<u32, pw::Error> {
    let node_id = Rc::new(OnceCell::new());
    let registry = core.get_registry()?;
    let node_id_clone = node_id.clone();
    let _listener = registry
        .add_listener_local()
        .global(move |global| {
            //let name: &str = &name;
            match global {
                GlobalObject {
                    props: Some(props), ..
                } => {
                    if let Some(node_name) = props.get(*pw::keys::NODE_NAME) {
                        if node_name == &name {
                            node_id_clone.set(global.id).unwrap();
                        }
                    }
                }
                _ => {}
            }
        })
        .register();

    do_roundtrip(&mainloop, &core);
    node_id.get().ok_or(pw::Error::NoMemory).copied()
}

/// Do a single roundtrip to process all events.
/// See the example in roundtrip.rs for more details on this.
fn do_roundtrip(mainloop: &pw::main_loop::MainLoopRc, core: &pw::core::CoreRc) {
    let done = Rc::new(Cell::new(false));
    let done_clone = done.clone();
    let loop_clone = mainloop.clone();

    // Trigger the sync event. The server's answer won't be processed until we start the main loop,
    // so we can safely do this before setting up a callback. This lets us avoid using a Cell.
    let pending = core.sync(0).expect("sync failed");

    let _listener_core = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == pw::core::PW_ID_CORE && seq == pending {
                done_clone.set(true);
                loop_clone.quit();
            }
        })
        .register();

    while !done.get() {
        mainloop.run();
    }
}

fn amp2db(amplitude: f32) -> f32 {
    let abs_amplitude = amplitude.abs();

    // Define a very small positive number to act as a floor.
    // This prevents `log10(0)` which is -infinity and causes NaN or inf.
    const MIN_AMPLITUDE: f32 = f32::EPSILON; // Smallest positive non-zero f64

    let clamped_amplitude = abs_amplitude.max(MIN_AMPLITUDE);

    // Apply the decibel formula: 20 * log10(clamped_amplitude / reference_amplitude)
    // Since we assume normalized amplitude, reference_amplitude is 1.0.
    20.0 * clamped_amplitude.log10()
}

pub fn hann_window(length: usize) -> impl Iterator<Item = f32> {
    (0..length).map(move |v| 0.5 * (1.0 - (TAU * v as f32 / (length as f32 - 1.0)).cos()))
}
