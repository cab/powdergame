import { mat4, ReadonlyVec3, vec3, vec4 } from 'gl-matrix'
import { initShaderProgram } from './glutil'
import { WasmModule } from './index'

interface Buffers {
  position: WebGLBuffer
  color: WebGLBuffer
}

export function render(wasm: WasmModule, canvas: HTMLCanvasElement) {
  let gl = canvas.getContext('webgl')
  if (!gl) {
    alert(
      'Unable to initialize WebGL. Your browser or machine may not support it.',
    )
    return
  }

  gl.viewport(0, 0, gl.canvas.width, gl.canvas.height)

  // Vertex shader program

  let vsSource = `
  attribute vec4 aVertexPosition;
   attribute vec2 aUv;
   
   varying lowp vec4 vColor;
   void main(void) {
     gl_Position = aVertexPosition - 0.5;
   }
  `

  // Fragment shader program

  let fsSource = `
   precision mediump float;
   

float distance_from_sphere(in vec3 p, in vec3 c, float r)
{
    return length(p - c) - r;
}

float map_the_world(in vec3 p)
{
    float sphere_0 = distance_from_sphere(p, vec3(0.0), 1.0);

    return sphere_0;
}

vec3 calculate_normal(in vec3 p)
{
    const vec3 small_step = vec3(0.001, 0.0, 0.0);

    float gradient_x = map_the_world(p + small_step.xyy) - map_the_world(p - small_step.xyy);
    float gradient_y = map_the_world(p + small_step.yxy) - map_the_world(p - small_step.yxy);
    float gradient_z = map_the_world(p + small_step.yyx) - map_the_world(p - small_step.yyx);

    vec3 normal = vec3(gradient_x, gradient_y, gradient_z);

    return normalize(normal);
}

vec3 ray_march(in vec3 ro, in vec3 rd)
{
    float total_distance_traveled = 0.0;
    const int NUMBER_OF_STEPS = 32;
    const float MINIMUM_HIT_DISTANCE = 0.001;
    const float MAXIMUM_TRACE_DISTANCE = 1000.0;

    for (int i = 0; i < NUMBER_OF_STEPS; ++i)
    {
        vec3 current_position = ro + total_distance_traveled * rd;

        float distance_to_closest = map_the_world(current_position);

        if (distance_to_closest < MINIMUM_HIT_DISTANCE) 
        {
            vec3 normal = calculate_normal(current_position);
            vec3 light_position = vec3(2.0, -5.0, 3.0);
            vec3 direction_to_light = normalize(current_position - light_position);

            float diffuse_intensity = max(0.0, dot(normal, direction_to_light));

            return vec3(1.0, 0.0, 0.0) * diffuse_intensity;
        }

        if (total_distance_traveled > MAXIMUM_TRACE_DISTANCE)
        {
            break;
        }
        total_distance_traveled += distance_to_closest;
    }
    return vec3(0.0);
}


void main()
{
    // TODO use actual canvas size
    vec2 vUv = vec2(gl_FragCoord.x / 1770.0, gl_FragCoord.y / 1330.0);
    vec2 uv = vUv * 2.0 - 1.0;

    vec3 camera_position = vec3(0.0, 0.0, -5.0);
    vec3 ro = camera_position;
    vec3 rd = vec3(uv, 1.0);

    vec3 shaded_color = ray_march(ro, rd);

    gl_FragColor = vec4(shaded_color, 1.0);

}
  `

  let shaderProgram = initShaderProgram(gl, vsSource, fsSource)

  if (!shaderProgram) {
    throw 'todo create shader'
  }

  gl.useProgram(shaderProgram)

  let fireAtlasUni = gl.getUniformLocation(shaderProgram, 'uFireAtlas')!
  let sdfUni = gl.getUniformLocation(shaderProgram, 'uSdf')!

  let projectionMatrix = gl.getUniformLocation(
    shaderProgram,
    'uProjectionMatrix',
  )!
  let modelViewMatrix = gl.getUniformLocation(
    shaderProgram,
    'uModelViewMatrix',
  )!

  let vertexPositionAttrib = gl.getAttribLocation(
    shaderProgram,
    'aVertexPosition',
  )
  let uvAttrib = gl.getAttribLocation(shaderProgram, 'aUv')

  let posBuffer = initBuffers(gl)
  let uvBuffer = initUvBuffer(gl)

  let sdf = wasm.march()
  let sdfTexture = createDataTexture(gl, sdf)

  let state: State = {
    wasm,
    render: {
      sdfUni,
      sdfTexture,
      uvAttrib,
      vertexPositionAttrib,
      posBuffer,

      uvBuffer,
      xRotation: 0,
      yRotation: 0,
      imageIsLoaded: false,
      shader: shaderProgram,
      lastMouseX: 0,
      lastMouseY: 0,
    },
  }

  {
    var fireTexture = gl.createTexture()
    var fireAtlas = new window.Image()
    fireAtlas.onload = function () {
      if (!gl) {
        alert('failed to load texture in context')
        return
      }
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true)
      gl.bindTexture(gl.TEXTURE_2D, fireTexture)
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        fireAtlas,
      )
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR)
      // todo event
      state.render.imageIsLoaded = true
    }
    fireAtlas.src = '/assets/rock.png'
  }

  gl.enable(gl.BLEND)
  gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA)

  // Push our fire texture atlas to the GPU
  gl.activeTexture(gl.TEXTURE0)
  gl.bindTexture(gl.TEXTURE_2D, fireTexture)
  gl.uniform1i(fireAtlasUni, 0)

  // sdf
  gl.activeTexture(gl.TEXTURE1)
  gl.bindTexture(gl.TEXTURE_2D, sdfTexture)
  gl.uniform1i(sdfUni, 1)

  canvas.addEventListener('mousemove', (e) => {
    // if (isDragging) {
    state.render.xRotation += (e.pageY - state.render.lastMouseY) / 50
    state.render.yRotation -= (e.pageX - state.render.lastMouseX) / 50

    state.render.xRotation = Math.min(state.render.xRotation, Math.PI / 2.5)
    state.render.xRotation = Math.max(state.render.xRotation, -Math.PI / 2.5)

    state.render.lastMouseX = e.pageX
    state.render.lastMouseY = e.pageY
    // }
  })

  draw(state, gl)
}

function initUvBuffer(gl: WebGLRenderingContext): WebGLBuffer {
  let textureCoordBuffer = gl.createBuffer()
  if (!textureCoordBuffer) {
    throw 'todo uv buffer'
  }
  gl.bindBuffer(gl.ARRAY_BUFFER, textureCoordBuffer)

  let textureCoordinates: number[] = []

  let width = 256
  let height = 256
  for (let x = 0; x < width; x++) {
    for (let y = 0; y < height; y++) {
      textureCoordinates.push(x / width)
      textureCoordinates.push(y / height)
    }
  }

  gl.bufferData(
    gl.ARRAY_BUFFER,
    new Float32Array(textureCoordinates),
    gl.STATIC_DRAW,
  )

  return textureCoordBuffer
}

function initBuffers(gl: WebGLRenderingContext): WebGLBuffer {
  // Create a buffer for the square's positions.

  let positionBuffer = gl.createBuffer()
  if (!positionBuffer) {
    throw 'todo buffer'
  }

  gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer)

  let x = 0
  let y = 0
  let width = 1
  let height = 1
  let x1 = x
  let x2 = x + width
  let y1 = y
  let y2 = y + height

  let positions = [x1, y1, x2, y1, x1, y2, x1, y2, x2, y1, x2, y2]

  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(positions), gl.STATIC_DRAW)

  return positionBuffer
}

function createCameraUniform(xRotation: number, yRotation: number) {
  var camera = mat4.create()

  // Start our camera off at a height of 0.25 and 1 unit
  // away from the origin
  mat4.translate(camera, camera, [0, 0.25, 1])

  // Rotate our camera around the y and x axis of the world
  // as the viewer clicks or drags their finger
  var xAxisRotation = mat4.create()
  var yAxisRotation = mat4.create()
  mat4.rotateX(xAxisRotation, xAxisRotation, -xRotation)
  mat4.rotateY(yAxisRotation, yAxisRotation, yRotation)
  mat4.multiply(camera, xAxisRotation, camera)
  mat4.multiply(camera, yAxisRotation, camera)

  // Make our camera look at the first red fire
  var cameraPos: ReadonlyVec3 = [camera[12], camera[13], camera[14]]
  mat4.lookAt(camera, cameraPos, [0, 0, 0], [0, 1, 0])

  return camera
}

interface State {
  wasm: WasmModule
  render: RenderState
}

interface GameState {}

interface RenderState {
  sdfTexture: WebGLBuffer
  shader: WebGLProgram
  posBuffer: WebGLBuffer
  uvBuffer: WebGLBuffer
  uvAttrib: number
  vertexPositionAttrib: number
  sdfUni: WebGLUniformLocation
  imageIsLoaded: boolean
  xRotation: number
  yRotation: number
  lastMouseX: number
  lastMouseY: number
}

function draw(state: State, gl: WebGLRenderingContext) {
  let render = state.render

  if (render.imageIsLoaded) {
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT)

    gl.useProgram(render.shader)

    {
      let buffers = render.posBuffer
      let numComponents = 2 // pull out 2 values per iteration
      let type = gl.FLOAT // the data in the buffer is 32bit floats
      let normalize = false // don't normalize
      let stride = 0 // how many bytes to get from one set of values to the next
      // 0 = use type and numComponents above
      let offset = 0 // how many bytes inside the buffer to start from
      gl.bindBuffer(gl.ARRAY_BUFFER, buffers)
      gl.vertexAttribPointer(
        render.vertexPositionAttrib,
        numComponents,
        type,
        normalize,
        stride,
        offset,
      )
      gl.enableVertexAttribArray(render.vertexPositionAttrib)
    }

    let offset = 0
    let vertexCount = 6
    gl.drawArrays(gl.TRIANGLES, offset, vertexCount)
  }

  // On the next animation frame we re-draw our particle effect
  window.requestAnimationFrame(() => draw(state, gl))
}

function createDataTexture(
  gl: WebGLRenderingContext,
  data: Float32Array,
): WebGLTexture {
  var ext = gl.getExtension('OES_texture_float')

  var texture = gl.createTexture()
  if (!texture) {
    throw 'todo create data'
  }
  gl.bindTexture(gl.TEXTURE_2D, texture)
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.FLOAT, data)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST)
  return texture
}
