#version 100
precision mediump float;

varying lowp vec2 uv;
uniform vec2 screenSize;
uniform sampler2D screenTexture;


void main() {
  vec2 new_uv = uv * (1.0 - uv.yx);
  float vig = new_uv.x * new_uv.y * radius;
  vig = pow(vig, extend);
  gl_FragColor = mix(color, texture2D(screenTexture, uv), vig);
}
