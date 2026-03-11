#include "nam_wrapper.h"

// For standard exceptions
#include <stdexcept>
#include <iostream> 
#include <filesystem>

// Include architectures to ensure they are registered
#include "wavenet.h"
#include "lstm.h"
#include "convnet.h"

static void ensure_architectures_registered() {
    auto& registry = nam::ConfigParserRegistry::instance();
    if (!registry.has("WaveNet")) {
        registry.registerParser("WaveNet", nam::wavenet::create_config);
    }
    if (!registry.has("LSTM")) {
        registry.registerParser("LSTM", nam::lstm::create_config);
    }
    if (!registry.has("ConvNet")) {
        registry.registerParser("ConvNet", nam::convnet::create_config);
    }
}

NamProcessWrapper::NamProcessWrapper(float sample_rate, int max_block_size)
    : m_sample_rate(sample_rate)
    , m_max_block_size(max_block_size)
{
    ensure_architectures_registered();
    // Initialize without a model.
    m_nam = nullptr;
}

void NamProcessWrapper::load_model(const std::string& model_path) {
    try {
        // Explicitly use std::filesystem::path to avoid ambiguity with json overload
        m_nam = nam::get_dsp(std::filesystem::path(model_path));
        
        // Finalize DSP object if not null
        if (m_nam != nullptr) {
            // Apply current sample rate and expected max block size
            m_nam->ResetAndPrewarm((double)m_sample_rate, m_max_block_size);
        }
    } catch (const std::exception& e) {
        std::cerr << "Failed to load NAM model: " << e.what() << "\n";
        m_nam = nullptr;
        throw; // Re-throw to inform caller if possible
    }
}

void NamProcessWrapper::load_model_content(const std::string& content) {
    try {
        nlohmann::json j = nlohmann::json::parse(content);
        m_nam = nam::get_dsp(j);
        
        if (m_nam != nullptr) {
            m_nam->ResetAndPrewarm((double)m_sample_rate, m_max_block_size);
        }
    } catch (const std::exception& e) {
        std::cerr << "Failed to load NAM model from content: " << e.what() << "\n";
        m_nam = nullptr;
        throw;
    }
}

void NamProcessWrapper::update_settings(float sample_rate, int max_block_size) {
    m_sample_rate = sample_rate;
    m_max_block_size = max_block_size;
    if (m_nam) {
        m_nam->ResetAndPrewarm((double)m_sample_rate, m_max_block_size);
    }
}

void NamProcessWrapper::process_block(const float* input, float* output, int num_frames) {
    if (m_nam == nullptr) {
         // Pass-through if no model is loaded
         for (int i = 0; i < num_frames; ++i) {
             output[i] = input[i];
         }
         return;
    }

    // NAM expects NAM_SAMPLE** (pointer to pointer to channel buffers)
    // For mono processing (common in NAM models):
    float* in_ptr = const_cast<float*>(input);
    float* out_ptr = output;
    
    float* in_ptrs[1] = { in_ptr };
    float* out_ptrs[1] = { out_ptr };
    
    m_nam->process(in_ptrs, out_ptrs, num_frames);
}


std::unique_ptr<NamProcessWrapper> new_nam_wrapper(float sample_rate, int max_block_size) {
    return std::make_unique<NamProcessWrapper>(sample_rate, max_block_size);
}
