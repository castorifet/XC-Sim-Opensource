#version 100
precision mediump float;

varying lowp vec2 uv;
uniform sampler2D screenTexture;


vec3 chromatic_slice(float t) {
  vec3 res = vec3(1.0 - t, 1.0 - abs(t - 1.0), t - 1.0);
  return max(res, 0.0);
}

void main() {
  vec3 sum = vec3(0.0);
  vec3 c = vec3(0.0);
  vec2 offset = (uv - vec2(0.5)) * vec2(1, -1);
  int sample_count = int(sampleCount);
  for (int i = 0; i < 64; ++i) {
    if (i >= sample_count) break;
    vec3 slice = vec3(1.0 - t, 1.0 - abs(t - 1.0), t - 1.0);
    slice = max(slice, 0.0);
    sum += slice;
    vec2 slice_offset = (t - 1.0) * power * offset;
    c += slice * texture2D(screenTexture, uv + slice_offset).rgb;
  }
  gl_FragColor.rgb = c / sum;
}
