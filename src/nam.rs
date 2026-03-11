#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("cpp/nam_wrapper.h");

        type NamProcessWrapper;

        // Factory function for instantiation
        fn new_nam_wrapper(sample_rate: f32, max_block_size: i32) -> UniquePtr<NamProcessWrapper>;

        // Load a model from a string path
        fn load_model(self: Pin<&mut NamProcessWrapper>, model_path: &CxxString) -> Result<()>;

        // Load model from memory
        fn load_model_content(self: Pin<&mut NamProcessWrapper>, content: &CxxString) -> Result<()>;

        // Process a block
        // Note: we'll have to deal with pointers safely in Rust
        unsafe fn process_block(
            self: Pin<&mut NamProcessWrapper>,
            input: *const f32,
            output: *mut f32,
            num_frames: i32,
        );

        // Update DSP settings
        fn update_settings(self: Pin<&mut NamProcessWrapper>, sample_rate: f32, max_block_size: i32);
    }
}

pub struct NamSynth {
    inner: cxx::UniquePtr<ffi::NamProcessWrapper>,
}

unsafe impl Send for NamSynth {}
unsafe impl Sync for NamSynth {}

impl NamSynth {
    pub fn new(sample_rate: f32, max_block_size: i32) -> Self {
        Self {
            inner: ffi::new_nam_wrapper(sample_rate, max_block_size),
        }
    }

    pub fn load_model(&mut self, path: &str) -> anyhow::Result<()> {
        cxx::let_cxx_string!(cxx_path = path);
        if !self.inner.is_null() {
            self.inner.pin_mut().load_model(&cxx_path)?;
        }
        Ok(())
    }

    pub fn load_model_content(&mut self, content: &str) -> anyhow::Result<()> {
        cxx::let_cxx_string!(cxx_content = content);
        if !self.inner.is_null() {
            self.inner.pin_mut().load_model_content(&cxx_content)?;
        }
        Ok(())
    }

    pub fn update_settings(&mut self, sample_rate: f32, max_block_size: i32) {
        if !self.inner.is_null() {
            self.inner.pin_mut().update_settings(sample_rate, max_block_size);
        }
    }

    pub fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        assert_eq!(input.len(), output.len());
        let num_frames = input.len() as i32;
        if !self.inner.is_null() {
            unsafe {
                self.inner.pin_mut().process_block(input.as_ptr(), output.as_mut_ptr(), num_frames);
            }
        }
    }
}
