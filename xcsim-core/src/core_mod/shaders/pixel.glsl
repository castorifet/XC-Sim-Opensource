#version 100
precision mediump float;

varying lowp vec2 uv;
uniform vec2 screenSize;
uniform sampler2D screenTexture;


void main() {
  vec2 factor = screenSize / size;
  float x = floor(uv.x * factor.x + 0.5) / factor.x;
  float y = floor(uv.y * factor.y + 0.5) / factor.y;
  gl_FragColor = texture2D(screenTexture, vec2(x, y));
}
