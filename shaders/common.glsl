// Hash / value-noise / fbm helpers shared by the simulation passes. No
// #version/precision pragma here — composeFragmentShader() prepends those.

float hash21(vec2 p) {
  p = fract(p * vec2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}

float noise2(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  float a = hash21(i);
  float b = hash21(i + vec2(1.0, 0.0));
  float c = hash21(i + vec2(0.0, 1.0));
  float d = hash21(i + vec2(1.0, 1.0));
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

float fbm(vec2 p, int octaves) {
  float sum = 0.0;
  float amp = 0.5;
  float freq = 1.0;
  for (int i = 0; i < 8; i++) {
    if (i >= octaves) break;
    sum += amp * noise2(p * freq);
    freq *= 2.0;
    amp *= 0.5;
  }
  return sum;
}

// fbm(p + warp·fbm2(p)) domain warping (Quilez) — the "folded layers" look
// behind the Field variation card's spatial feed/kill perturbation.
float warpedFbm(vec2 p, float warp, int octaves) {
  vec2 q = vec2(fbm(p, octaves), fbm(p + vec2(5.2, 1.3), octaves));
  vec2 r = p + warp * q;
  return fbm(r, octaves);
}

// 2D curl of a scalar fbm potential via central differences: divergence-
// free, so advected material neither bunches up nor evaporates.
vec2 curlNoise(vec2 p, float eps) {
  float n1 = fbm(p + vec2(0.0, eps), 4);
  float n2 = fbm(p - vec2(0.0, eps), 4);
  float n3 = fbm(p + vec2(eps, 0.0), 4);
  float n4 = fbm(p - vec2(eps, 0.0), 4);
  float dx = (n3 - n4) / (2.0 * eps);
  float dy = (n1 - n2) / (2.0 * eps);
  return vec2(dy, -dx);
}
