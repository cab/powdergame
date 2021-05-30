import { mat4, ReadonlyVec3, vec3, vec4 } from 'gl-matrix'
import { initShaderProgram } from './glutil'
import { WasmModule } from './index'

let fsSource = require('./frag.glsl')
let vsSource = require('./vert.glsl')

export function render(wasm: WasmModule, canvas: HTMLCanvasElement) {
  let gl = canvas.getContext('webgl2')
  if (!gl) {
    alert(
      'Unable to initialize WebGL. Your browser or machine may not support it.',
    )
    return
  }

  gl.viewport(0, 0, gl.canvas.width, gl.canvas.height)

  // Vertex shader program

  let shaderProgram = initShaderProgram(gl, vsSource, fsSource)

  if (!shaderProgram) {
    throw 'todo create shader'
  }

  gl.useProgram(shaderProgram)

  let samplerUni = gl.getUniformLocation(shaderProgram, 'u_sampler')!
  let sdfUni = gl.getUniformLocation(shaderProgram, 'u_sdf')!

  let cameraPosition = gl.getUniformLocation(
    shaderProgram,
    'u_camera_position',
  )!

  let cameraDirection = gl.getUniformLocation(
    shaderProgram,
    'u_camera_direction',
  )!

  let vertexPositionAttrib = gl.getAttribLocation(
    shaderProgram,
    'aVertexPosition',
  )

  let posBuffer = initBuffers(gl)

  let sdf = wasm.create_sdf()
  let sdfTexture = createDataTexture(gl, sdf)

  let fireTexture = gl.createTexture()
  if (!fireTexture) {
    throw 'todo fix tex'
  }

  {
    gl.activeTexture(gl.TEXTURE0)
    gl.bindTexture(gl.TEXTURE_2D, fireTexture)
    gl.texImage2D(
      gl.TEXTURE_2D,
      0,
      gl.RGBA,
      1,
      1,
      0,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      new Uint8Array([0, 0, 255, 255]),
    )

    let fireAtlas = new window.Image()
    fireAtlas.addEventListener('load', function () {
      if (!gl) {
        alert('failed to load texture in context')
        return
      }
      gl.activeTexture(gl.TEXTURE0)
      // gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true)
      gl.bindTexture(gl.TEXTURE_2D, fireTexture)
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        fireAtlas,
      )
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT)
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT)
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR)
      state.render.imageIsLoaded = true
    })
    fireAtlas.src = '/assets/rock2.png'
  }

  let state: State = {
    wasm,
    render: {
      sdfUni,
      sdfTexture,
      vertexPositionAttrib,
      cameraPosition,
      cameraDirection,
      posBuffer,
      samplerUni,

      fireTexture,
      xRotation: 0,
      yRotation: 0,
      imageIsLoaded: false,
      shader: shaderProgram,
      lastMouseX: 0,
      lastMouseY: 0,
    },
  }

  gl.enable(gl.BLEND)
  gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA)

  // sdf
  // gl.activeTexture(gl.TEXTURE1)
  // gl.bindTexture(gl.TEXTURE_2D, sdfTexture)
  // gl.uniform1i(sdfUni, 1)

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

function initBuffers(gl: WebGL2RenderingContext): WebGLBuffer {
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

function createCameraUniform(xRotation: number, yRotation: number): mat4 {
  let camera = mat4.create()

  // Start our camera off at a height of 0.25 and 1 unit
  // away from the origin
  mat4.translate(camera, camera, [0, 0.25, 1])

  // Rotate our camera around the y and x axis of the world
  // as the viewer clicks or drags their finger
  let xAxisRotation = mat4.create()
  let yAxisRotation = mat4.create()
  mat4.rotateX(xAxisRotation, xAxisRotation, -xRotation)
  mat4.rotateY(yAxisRotation, yAxisRotation, yRotation)
  mat4.multiply(camera, xAxisRotation, camera)
  mat4.multiply(camera, yAxisRotation, camera)

  // Make our camera look at the first red fire
  let cameraPos: ReadonlyVec3 = [camera[12], camera[13], camera[14]]
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
  samplerUni: WebGLUniformLocation
  vertexPositionAttrib: number
  sdfUni: WebGLUniformLocation
  fireTexture: WebGLTexture
  cameraPosition: WebGLUniformLocation
  cameraDirection: WebGLUniformLocation
  imageIsLoaded: boolean
  xRotation: number
  yRotation: number
  lastMouseX: number
  lastMouseY: number
}

function draw(state: State, gl: WebGL2RenderingContext) {
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

    {
      gl.uniform1i(render.samplerUni, 0)
      // Tell WebGL we want to affect texture unit 0
      gl.activeTexture(gl.TEXTURE0)

      // Bind the texture to texture unit 0
      gl.bindTexture(gl.TEXTURE_2D, render.fireTexture)

      // Tell the shader we bound the texture to texture unit 0
    }

    gl.uniform3fv(render.cameraPosition, [0, 0, -6])
    gl.uniform3fv(render.cameraDirection, [0, 0, 1.0])

    let offset = 0
    let vertexCount = 6
    gl.drawArrays(gl.TRIANGLES, offset, vertexCount)
  }

  // On the next animation frame we re-draw our particle effect
  window.requestAnimationFrame(() => draw(state, gl))
}

function createDataTexture(
  gl: WebGL2RenderingContext,
  data: Float32Array,
): WebGLTexture {
  let texture = gl.createTexture()
  if (!texture) {
    throw 'todo create data'
  }
  gl.bindTexture(gl.TEXTURE_2D, texture)
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGB16F, 1, 1, 0, gl.RGB, gl.FLOAT, data)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST)
  return texture
}
