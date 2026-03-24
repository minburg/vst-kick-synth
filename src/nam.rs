#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("cpp/nam_wrapper.h");

        type NamProcessWrapper;

        // Returns Result so any C++ exception during construction is caught by
        // cxx and propagated as a Rust error instead of calling std::terminate().
        fn new_nam_wrapper(sample_rate: f32, max_block_size: i32) -> Result<UniquePtr<NamProcessWrapper>>;

        fn load_model(self: Pin<&mut NamProcessWrapper>, model_path: &CxxString) -> Result<()>;
        fn load_model_content(self: Pin<&mut NamProcessWrapper>, content: &CxxString) -> Result<()>;

        // Returns Result so a C++ exception during audio processing falls back
        // gracefully rather than terminating the host process.
        unsafe fn process_block(
            self: Pin<&mut NamProcessWrapper>,
            input: *const f32,
            output: *mut f32,
            num_frames: i32,
        ) -> Result<()>;

        // Returns Result so ResetAndPrewarm exceptions are caught.
        fn update_settings(self: Pin<&mut NamProcessWrapper>, sample_rate: f32, max_block_size: i32) -> Result<()>;
    }
}

pub struct NamSynth {
    inner: Option<cxx::UniquePtr<ffi::NamProcessWrapper>>,
}

unsafe impl Send for NamSynth {}
unsafe impl Sync for NamSynth {}

impl NamSynth {
    pub fn new(sample_rate: f32, max_block_size: i32) -> Self {
        match ffi::new_nam_wrapper(sample_rate, max_block_size) {
            Ok(ptr) => Self { inner: Some(ptr) },
            Err(e) => {
                // Log and fall back to a null wrapper. All methods guard with
                // if-let Some so subsequent calls become silent no-ops.
                eprintln!("[kick_synth] Failed to initialize NAM wrapper: {e}");
                Self { inner: None }
            }
        }
    }

    pub fn load_model(&mut self, path: &str) -> anyhow::Result<()> {
        cxx::let_cxx_string!(cxx_path = path);
        if let Some(ref mut inner) = self.inner {
            inner.pin_mut().load_model(&cxx_path)?;
        }
        Ok(())
    }

    pub fn load_model_content(&mut self, content: &str) -> anyhow::Result<()> {
        cxx::let_cxx_string!(cxx_content = content);
        if let Some(ref mut inner) = self.inner {
            inner.pin_mut().load_model_content(&cxx_content)?;
        }
        Ok(())
    }

    pub fn update_settings(&mut self, sample_rate: f32, max_block_size: i32) {
        if let Some(ref mut inner) = self.inner {
            if let Err(e) = inner.pin_mut().update_settings(sample_rate, max_block_size) {
                eprintln!("[kick_synth] NAM update_settings error: {e}");
            }
        }
    }

    pub fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        assert_eq!(input.len(), output.len());
        let num_frames = input.len() as i32;
        if let Some(ref mut inner) = self.inner {
            // SAFETY: input/output are valid slices of length num_frames.
            let result = unsafe {
                inner.pin_mut().process_block(input.as_ptr(), output.as_mut_ptr(), num_frames)
            };
            if result.is_err() {
                // Audio thread — no heap allocation / logging allowed.
                // Fall back to pass-through so the host does not hear silence.
                output.copy_from_slice(input);
            }
        }
    }
}
