#pragma once

#include "NAM/dsp.h"
#include "NAM/get_dsp.h"
#include <memory>
#include <string>

/// Real-time capable wrapper for NAM Core DSP
class NamProcessWrapper {
public:
    NamProcessWrapper(float sample_rate, int max_block_size);
    ~NamProcessWrapper() = default;

    /// Load a .nam file. This involves allocations and should NOT be called on the audio thread!
    void load_model(const std::string& model_path);

    /// Process a block of audio. Safe to call on the audio thread.
    void process_block(const float* input, float* output, int num_frames);

    /// Change sample rate or maximal block size. (Not real-time safe!).
    void update_settings(float sample_rate, int max_block_size);

private:
    float m_sample_rate;
    int m_max_block_size;
    
    // The underlying NAM DSP object
    std::unique_ptr<nam::DSP> m_nam;
};

// Functions to expose to Rust via cxx
std::unique_ptr<NamProcessWrapper> new_nam_wrapper(float sample_rate, int max_block_size);
