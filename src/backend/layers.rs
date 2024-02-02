use candle_core::Tensor;
use either::Either;

use crate::{
    backend::{get_or_load_func, ROTARY_EMBDEDDING_KERNEL, ROTARY_EMBDEDDING_PTX},
    try_api,
};

use super::dispatch_get_cuda_pointer;

pub fn rotary_embedding(
    positions: Tensor,
    query: &mut Tensor,
    key: &mut Tensor,
    head_size: usize,
    cos_sin_cache: Tensor,
    is_neox: bool,
) {
    let positions_dev = positions.device();
    let Device::Cuda(dev) = positions_dev else {
        panic!("Expected the positions to be on a CUDA device.")
    };

    if !query.device().same_device(positions.device()) {
        return Err(APIError::new(format!(
            "`query` and `positions` have different devices, got {:?} and {:?} respectively.",
            query.device(),
            positions.device()
        )));
    }

    if !key.device().same_device(positions.device()) {
        return Err(APIError::new(format!(
            "`key` and `positions` have different devices, got {:?} and {:?} respectively.",
            key.device(),
            positions.device()
        )));
    }

    if !cos_sin_cache.device().same_device(positions.device()) {
        return Err(APIError::new(format!(
            "`cos_sin_cache` and `positions` have different devices, got {:?} and {:?} respectively.",
            cos_sin_cache.device(),
            positions.device()
        )));
    }

    let num_tokens = query.shape().elem_count() / query.shape().dims().last().unwrap();
    let rot_dim = cos_sin_cache.shape().dims().get(1).unwrap();
    let num_heads = query.shape().dims().last().unwrap() / head_size;
    let num_kv_heads = key.shape().dims().last().unwrap() / head_size;
    let query_stride = query.stride().get(key.stride().len() - 2).unwrap();
    let key_stride = key.stride().get(key.stride().len() - 2).unwrap();

    let launch_conf = LaunchConfig {
        grid_dim: (num_tokens.try_into().unwrap(), 1u32, 1u32),
        block_dim: (
            512.min((num_heads * rot_dim / 2).try_into().unwrap()),
            1u32,
            1u32,
        ),
        shared_mem_bytes: 0,
    };

    let positions_ptr = dispatch_get_cuda_pointer(positions);
    let key_ptr = dispatch_get_cuda_pointer(key);
    let query_ptr = dispatch_get_cuda_pointer(query);
    let cos_sin_cache_ptr = dispatch_get_cuda_pointer(cos_sin_cache);

    let stream = try_api!(dev.fork_default_stream());

    let kernel = if is_neox {
        try_api!(get_or_load_func(
            ROTARY_EMBDEDDING_PTX,
            ROTARY_EMBDEDDING_KERNEL,
            Either::Right("_neox"),
            dev
        ))
    } else {
        try_api!(get_or_load_func(
            ROTARY_EMBDEDDING_PTX,
            ROTARY_EMBDEDDING_KERNEL,
            Either::Right("_normal"),
            dev
        ))
    };

    try_api!(unsafe {
        kernel.launch_on_stream(
            &stream,
            launch_conf,
            (
                positions_ptr,
                query_ptr,
                key_ptr,
                cos_sin_cache_ptr,
                rot_dim,
                query_stride,
                key_stride,
                num_heads,
                num_kv_heads,
                head_size,
            ),
        )
    });

    /*
    const at::cuda::OptionalCUDAGuard device_guard(device_of(query));
    const cudaStream_t stream = at::cuda::getCurrentCUDAStream();
    VLLM_DISPATCH_FLOATING_TYPES(
      query.scalar_type(),
      "rotary_embedding",
      [&] {
        if (is_neox) {
          vllm::rotary_embedding_kernel<scalar_t, true><<<grid, block, 0, stream>>>(
            positions.data_ptr<int64_t>(),
            query.data_ptr<scalar_t>(),
            key.data_ptr<scalar_t>(),
            cos_sin_cache.data_ptr<scalar_t>(),
            rot_dim,
            query_stride,
            key_stride,
            num_heads,
            num_kv_heads,
            head_size);
        } else {
          vllm::rotary_embedding_kernel<scalar_t, false><<<grid, block, 0, stream>>>(
            positions.data_ptr<int64_t>(),
            query.data_ptr<scalar_t>(),
            key.data_ptr<scalar_t>(),
            cos_sin_cache.data_ptr<scalar_t>(),
            rot_dim,
            query_stride,
            key_stride,
            num_heads,
            num_kv_heads,
            head_size);
        }
      });*/
}
