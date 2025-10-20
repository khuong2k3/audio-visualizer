use crossterm::{cursor, style, terminal, QueueableCommand};
use pipewire as pw;
use pipewire::registry::GlobalObject;
use pipewire::spa::param::format::{MediaSubtype, MediaType};
use pipewire::spa::param::format_utils;
use pw::{properties::properties, spa};
use rustfft::FftPlanner;
use rustfft::num_complex::{Complex32, ComplexFloat};
use spa::pod::Pod;
use std::cell::{Cell, OnceCell};
use std::io::{Write, stdout};
use std::rc::Rc;
use std::sync::mpsc;
use std::{mem, slice};

mod buffer;

const FIRE_STRING: [char; 52] = [
    ' ', ',', ';', '+', 'l', 't', 'g', 't', 'i', '!', 'l', 'I', '?', '/', '\\', '|', ')', '(', '1',
    '}', '{', ']', '[', 'r', 'c', 'v', 'z', 'j', 'f', 't', 'J', 'U', 'O', 'Q', 'o', 'c', 'x', 'f',
    'X', 'h', 'q', 'w', 'W', 'B', '8', '&', '%', '$', '#', '@', '"', ';',
];

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
const MAX_DB: f32 = -10.0;

fn min_max_norm(n: f32, min_v: f32, max_v: f32) -> f32 {
    (n - min_v) / (max_v - min_v)
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

    let mut fft = FftPlanner::<f32>::new();
    let fft_forward = fft.plan_fft(256, rustfft::FftDirection::Forward);
    let mut fft_buffer = vec![Complex32::default(); 256];
    let mut fft_scratch = fft_buffer.clone();
    //fft_forward.process_with_scratch(buffer, scratch);

    let props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        // Stream/Audio/Input why this is correct ???
        *pw::keys::MEDIA_CLASS => "Stream/Audio/Input",
        *pw::keys::AUDIO_FORMAT => "F32LE",
        "audio.quantum" => "1024", // Set the processing block size
        *pw::keys::NODE_LATENCY => format!("{}/{}", 1024, 48_000), // Request latency
    };

    let stream = pw::stream::StreamBox::new(&core, FILTER_NAME, props)?;

    stdout()
        .queue(terminal::Clear(terminal::ClearType::All))
        .unwrap()
        .queue(cursor::MoveTo(0, 0))
        .unwrap()
        .queue(cursor::Hide)
        .unwrap()
        .flush()
        .unwrap();

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
            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }
                let data = &mut datas[0];
                let n_channels = user_data.format.channels();

                if let Some(samples) = data.data() {
                    //if user_data.cursor_move {
                    //    print!("\x1B[{}A", n_channels + 1);
                    //}
                    let samples = unsafe { bytes_to::<f32>(samples) };
                    //let n_samples = samples.len() as u32;
                    //println!("captured {} samples", n_samples / n_channels);

                    let samples_mono = samples
                        .chunks(n_channels as usize)
                        .map(|s| s.iter().sum::<f32>() / n_channels as f32)
                        .take(256);

                    fft_buffer
                        .iter_mut()
                        .zip(samples_mono)
                        .for_each(|(fft_buf, re)| *fft_buf = Complex32 { re, im: 0.0 });
                    fft_forward.process_with_scratch(&mut fft_buffer, &mut fft_scratch);

                    let mut stdout = stdout();
                    stdout
                        .queue(terminal::Clear(terminal::ClearType::All))
                        .unwrap();

                    for (i, amp) in fft_buffer
                        .iter()
                        .map(|n| min_max_norm(20.0 * n.abs().log10(), MIN_DB, MAX_DB))
                        .enumerate()
                    {
                        let amp = (amp * 20.0) as usize;
                        for height in 0..30 {
                            if amp > height {
                                stdout
                                    .queue(cursor::MoveTo(i as u16, height as u16))
                                    .unwrap()
                                    .queue(style::Print("0"))
                                    .unwrap();
                            }
                        }
                    }
                    stdout.flush().unwrap();

                    //stdout()
                    //    .queue(terminal::Clear(terminal::ClearType::All)).unwrap()
                    //    .queue(cursor::MoveTo(0, 0)).unwrap()
                    //    .flush().unwrap()

                    //for c in 0..n_channels {
                    //    let mut max: f32 = 0.0;
                    //    for n in (c..n_samples).step_by(n_channels as usize) {
                    //        let f = samples[n as usize];
                    //        //max = max.max((f as f32 / i16::MAX as f32).abs());
                    //        max = max.max(f.abs());
                    //    }
                    //
                    //    let peak = ((max * 30.0) as usize).clamp(0, 39);
                    //
                    //    println!(
                    //        "channel {}: |{:>w1$}{:w2$}| peak:{}",
                    //        c,
                    //        "*",
                    //        "",
                    //        max,
                    //        w1 = peak + 1,
                    //        w2 = 40 - peak
                    //    );
                    //}
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

    let (sx, rx) = mpsc::channel();
    sx.send(0).unwrap();
    //let mainloop_clone = mainloop.clone();
    //ctrlc::set_handler(move || {
    //    stdout().execute(cursor::Show).unwrap();
    //    //mainloop_clone.quit();
    //    sx.send(1).unwrap();
    //}).unwrap();

    // and wait while we let things run
    mainloop.run();

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
