//! 仅提取固定诊断标记；绝不把模型路径、提示词或原始 stderr 传出。

#[derive(Default)]
pub(super) struct Diagnostics {
    pub backend: Option<&'static str>,
    pub gpu_failed: bool,
}

impl Diagnostics {
    /// 只接受后端使用记录，不把编译能力或设备枚举误当成实际后端。
    pub fn observe(&mut self, line: &str) {
        let line = line.to_ascii_lowercase();
        let diagnostic = line.starts_with("ggml_")
            || line.starts_with("whisper_")
            || line.starts_with("cuda error")
            || line.starts_with("vulkan error")
            || line.starts_with("metal error");
        if !diagnostic {
            return;
        }
        let gpu = ["vulkan", "cuda", "metal", "sycl", "hip", "rocm", "opencl"]
            .into_iter()
            .find(|name| line.contains(name));
        let failed = ["failed", "error", "out of memory", "cannot allocate"]
            .iter()
            .any(|marker| line.contains(marker));
        // 仅同一条后端诊断中的明确错误触发 GPU 回退；通用模型加载失败不算。
        let origin = line.split(':').next().unwrap_or("");
        let gpu_origin = [
            "ggml_vulkan",
            "ggml_cuda",
            "ggml_metal",
            "ggml_sycl",
            "ggml_hip",
            "ggml_opencl",
            "ggml_vk_",
            "ggml_backend_vk",
            "ggml_backend_cuda",
            "ggml_backend_metal",
            "ggml_backend_sycl",
            "ggml_backend_hip",
            "ggml_backend_opencl",
            "cuda error",
            "vulkan error",
            "metal error",
        ]
        .iter()
        .any(|prefix| origin.starts_with(prefix));
        let backend_init_failed =
            matches!(origin, "whisper_backend_init" | "whisper_backend_init_gpu")
                && (line.contains("failed to initialize gpu")
                    || line.contains("failed to init gpu")
                    || (gpu.is_some() && line.contains("backend")));
        if failed && (gpu_origin || backend_init_failed) {
            self.gpu_failed = true;
        }
        if failed {
            return;
        }
        let selected = (line.contains("using ") && line.contains(" backend"))
            || line.contains("using device")
            || line.contains("using backend")
            || line.contains("selected backend")
            || line.contains("buffer size")
            || line.contains("compute buffer")
            || (origin == "whisper_model_load" && line.contains("total size"));
        if selected {
            if let Some(gpu) = gpu {
                self.backend = Some(gpu);
            } else if line.contains("cpu") && self.backend.is_none() {
                // GPU 运行也会分配 CPU 缓冲区，不能用后者覆盖已确认的加速后端。
                self.backend = Some("cpu");
            }
        }
        if line.contains("using cpu backend") || line.contains("falling back to cpu") {
            self.backend = Some("cpu");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Diagnostics;

    /// 模型路径和能力枚举既不证明 GPU 已启用，也不证明 GPU 出错。
    #[test]
    fn ignores_untrusted_text_and_capabilities() {
        let mut state = Diagnostics::default();
        for line in [
            "error loading /cuda/model.bin",
            "system_info: CUDA = 1",
            "ggml_cuda_init: found 1 CUDA devices",
            "whisper_init: failed to load model",
            "whisper_init_from_file_with_params: failed to open /cuda/model.bin",
        ] {
            state.observe(line);
        }
        assert_eq!(state.backend, None);
        assert!(!state.gpu_failed);
    }

    /// 已确认的 GPU 不会被共享 CPU 缓冲区覆盖，显式回退除外。
    #[test]
    fn reports_allowlisted_selected_backend() {
        let mut state = Diagnostics::default();
        state.observe("whisper_backend_init: using Vulkan backend");
        // 未知格式不能猜测；典型缓冲区记录可作为实际分配证据。
        state.observe("whisper_init_state: Vulkan0 compute buffer = 12 MB");
        state.observe("whisper_init_state: CPU compute buffer = 2 MB");
        assert_eq!(state.backend, Some("vulkan"));
        state.observe("whisper_backend_init: falling back to CPU");
        assert_eq!(state.backend, Some("cpu"));
    }

    /// v1.7.6 CPU 构建在模型缓冲区打印 CPU，计算缓冲区不再带设备名。
    #[test]
    fn recognizes_real_cpu_allocation_log() {
        let mut state = Diagnostics::default();
        state.observe("whisper_model_load:          CPU total size =   487.01 MB");
        state.observe("whisper_backend_init_gpu: no GPU found");
        state.observe("whisper_init_state: compute buffer (encode) = 85 MB");
        assert_eq!(state.backend, Some("cpu"));
        assert!(!state.gpu_failed);
        state.observe("whisper_backend_init_gpu: using Vulkan0 backend");
        assert_eq!(state.backend, Some("vulkan"));
        state.observe("whisper_backend_init_gpu: failed to initialize Vulkan0 backend");
        assert!(state.gpu_failed);
    }

    /// 只有明确关联后端的失败诊断才标为 GPU 故障。
    #[test]
    fn recognizes_backend_failure() {
        let mut state = Diagnostics::default();
        for line in [
            "ggml_vulkan: failed to allocate device memory",
            "ggml_backend_cuda_buffer_type_alloc_buffer: allocating device memory failed",
            "ggml_backend_vk_buffer_type_alloc_buffer: failed to allocate buffer",
            "whisper_backend_init: failed to initialize GPU backend",
        ] {
            state.gpu_failed = false;
            state.observe(line);
            assert!(state.gpu_failed);
            assert_eq!(state.backend, None);
        }
    }
}
