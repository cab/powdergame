void main() {
  gl_FragColor = vec4(gl_FragCoord.xy / 1000.0, 1.0, 1.0);
}