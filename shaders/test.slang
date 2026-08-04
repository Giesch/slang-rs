StructuredBuffer<float> input_0;
StructuredBuffer<float> input_1;
RWStructuredBuffer<float> output;

Texture2DArray<float> tex_array;
Sampler2D combined_sampler;
Sampler2DShadow shadow_sampler;

[shader("compute")]
[numthreads(1, 1, 1)]
void main(uint3 thread_id : SV_DispatchThreadID) {
	let index = thread_id.x;
	let uv = float2(thread_id.xy);

	let sampled = tex_array.Load(int4(thread_id, 0))
		+ combined_sampler.SampleLevel(uv, 0).x
		+ shadow_sampler.SampleCmpLevelZero(uv, 0.5);

	output[index] = input_0[index] * input_1[index] + sampled;
}
