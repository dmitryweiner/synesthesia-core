// Field -> color. THE ONE PASS THAT DOES NOT SHRINK WITH `?res=`: it runs
// once per canvas pixel, so the canvas backing store (src/sim/quality.ts)
// is what bounds it, and the exp/pow it used to do per pixel are written
// out as multiplies — a scalar rasterizer pays for those, a GPU does not.
//
// Field -> color: an iq-style cosine gradient (a/b/c/d already carry the
// Palette card's shift/contrast — see src/palette.ts composePalette), plus a
// bump-mapped relief pass that makes a flat gradient lookup read as "stone"
// or "wet glaze" rather than a heatmap.
// Explicit sound → image effects (src/visualFx.ts, PLAN.md decision 8) act
// here, in the same frame the sound happens:
//   uRipples  — up to 4 rings (x, y, age, amp) spreading from onset hits;
//               they displace the lookup like a wave on water and brighten
//               their crest
//   uTint     — bass / mid / treble levels tint dark / mid / light tones
//   uFlash    — onset envelope flares the highlights
//   uExposure — loudness swell breathes the brightness
in vec2 vUv;
out vec4 fragColor;

uniform sampler2D uState;
uniform vec2 uTexel;
uniform vec3 uPalA;
uniform vec3 uPalB;
uniform vec3 uPalC;
uniform vec3 uPalD;
uniform float uBands;
uniform float uRelief;
uniform vec3 uLightDir; // normalized on the CPU — sin/cos of a uniform, per pixel, is not free
uniform float uGloss;
uniform float uAspect;
uniform float uExposure;
uniform float uFlash;
uniform vec3 uTint;
uniform vec4 uRipples[4];

const float RIPPLE_SPEED = 0.35;  // UV (height units) per second
const float RIPPLE_WIDTH = 0.035;
const float RIPPLE_LIFE = 1.6;    // must match src/visualFx.ts
const float RIPPLE_PUSH = 0.012;  // max displacement, UV
const vec3 TINT_LOW = vec3(0.85, 0.18, 0.30);
const vec3 TINT_MID = vec3(1.0, 0.72, 0.25);
const vec3 TINT_HIGH = vec3(0.55, 0.90, 1.0);

vec3 cosinePalette(float t, vec3 a, vec3 b, vec3 c, vec3 d) {
  return a + b * cos(6.28318 * (c * t + d));
}

float heightAt(vec2 uv) {
  return texture(uState, uv).g;
}

void main() {
  // ripples: radial displacement of the lookup + a bright crest
  vec2 uv = vUv;
  float crest = 0.0;
  for (int i = 0; i < 4; i++) {
    vec4 r = uRipples[i];
    if (r.w <= 0.0) continue;
    vec2 d = (vUv - r.xy) * vec2(uAspect, 1.0);
    float dist = length(d);
    float g = (dist - r.z * RIPPLE_SPEED) / RIPPLE_WIDTH;
    float ring = exp(-g * g);
    float fade = r.w * (1.0 - r.z / RIPPLE_LIFE);
    float wave = ring * fade;
    vec2 dir = dist > 1e-5 ? d / dist : vec2(0.0);
    uv -= dir / vec2(uAspect, 1.0) * wave * RIPPLE_PUSH;
    crest += wave;
  }

  vec4 s = texture(uState, uv);
  float raw = clamp(s.g * 1.6, 0.0, 1.0);
  float t = fract(raw * uBands);
  vec3 col = cosinePalette(t, uPalA, uPalB, uPalC, uPalD);

  // spectrum tint by tone: bass → dark tones, mids → middle, treble → light
  float wLow = 1.0 - smoothstep(0.0, 0.45, raw);
  float wHigh = smoothstep(0.55, 1.0, raw);
  float wMid = clamp(1.0 - wLow - wHigh, 0.0, 1.0);
  col = mix(col, TINT_LOW, uTint.x * wLow * 0.6);
  col = mix(col, TINT_MID, uTint.y * wMid * 0.6);
  col = mix(col, TINT_HIGH, uTint.z * wHigh * 0.6);

  float hL = heightAt(uv - vec2(uTexel.x, 0.0));
  float hR = heightAt(uv + vec2(uTexel.x, 0.0));
  float hD = heightAt(uv - vec2(0.0, uTexel.y));
  float hU = heightAt(uv + vec2(0.0, uTexel.y));
  vec3 normal = normalize(vec3((hL - hR) * uRelief, (hD - hU) * uRelief, 1.0));

  float diffuse = max(dot(normal, uLightDir), 0.0);
  vec3 reflectDir = reflect(-uLightDir, normal);
  float z = max(reflectDir.z, 0.0);
  float z2 = z * z;
  float z4 = z2 * z2;
  float z8 = z4 * z4;
  float specular = z8 * z8 * z8 * (uGloss + uFlash * 1.5); // pow(z, 24), unrolled

  col *= 0.55 + 0.55 * diffuse;
  col += specular;
  col += uFlash * 0.18 + crest * 0.22;
  col *= uExposure;

  fragColor = vec4(col, 1.0);
}
