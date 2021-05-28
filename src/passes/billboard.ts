import { mat4, ReadonlyVec3, vec3, vec4 } from 'gl-matrix'
import { WasmModule } from '../index'

export function createPass() {}

export function render(wasm: WasmModule, gl: WebGLRenderingContext) {
  let vsSource = `

uniform vec3 uFirePos;

attribute vec2 aTextureCoords;

attribute vec2 aTriCorner;

attribute vec3 aCenterOffset;

uniform mat4 uPMatrix;
uniform mat4 uViewMatrix;

varying vec2 vTextureCoords;

varying float v_w;

void main (void) {

  vec4 position = vec4(
    uFirePos + aCenterOffset,
    1.0
  );

  float size = 0.05;

  vec3 cameraRight = vec3(
    uViewMatrix[0].x, uViewMatrix[1].x, uViewMatrix[2].x
  );
  vec3 cameraUp = vec3(
    uViewMatrix[0].y, uViewMatrix[1].y, uViewMatrix[2].y
  );

  position.xyz += (cameraRight * aTriCorner.x * size) +
    (cameraUp * aTriCorner.y * size);
  
  gl_Position = uPMatrix * uViewMatrix * position;
  v_w = 1.0 / gl_Position.w;

  vTextureCoords = aTextureCoords;

   if (gl_Position.w > 0.0) {
    gl_PointSize = 32.0 / gl_Position.w;
  } else {
    gl_PointSize = 0.0;
  }
}
  `

  // Fragment shader program

  let fsSource = `
   precision mediump float;

varying float v_w;

varying vec2 vTextureCoords;
uniform sampler2D fireAtlas;
uniform sampler2D sdf;

const vec4 begin = vec4(0.1, 0.75, 1.0, 1.0);
const vec4 end = vec4(1.0, 1.0, 1.0, 1.0);

vec4 interpolate4f(vec4 a,vec4 b, float p) {
  return p * b + (1.0 - p) * a;
}

void main(void) {

  vec2 pc = (gl_PointCoord - 0.5) * 2.0;

  float dist = (1.0 - sqrt(pc.x * pc.x + pc.y * pc.y));
  vec4 color = interpolate4f(begin, end, dist);
  vec4 texColor2 = vec4(dist, dist, dist, dist * dist * v_w) * color;

  vec4 texColor = texture2D(
    fireAtlas, 
    vec2(
      gl_PointCoord.x / 2.0,
      gl_PointCoord.y / 2.0
  ));

  if(texColor.a < 0.1) {
    discard;
  }

  gl_FragColor = texColor;

}
  `

  let shaderProgram = initShaderProgram(gl, vsSource, fsSource)

  if (!shaderProgram) {
    throw 'todo'
  }

  gl.useProgram(shaderProgram)

  let firePosUni = gl.getUniformLocation(shaderProgram, 'uFirePos')!
  let perspectiveUni = gl.getUniformLocation(shaderProgram, 'uPMatrix')!
  let viewUni = gl.getUniformLocation(shaderProgram, 'uViewMatrix')!
  let fireAtlasUni = gl.getUniformLocation(shaderProgram, 'uFireAtlas')!
  let sdfUni = gl.getUniformLocation(shaderProgram, 'uSdf')!

  let texCoordAttrib = gl.getAttribLocation(shaderProgram, 'aTextureCoords')
  let triCornerAttrib = gl.getAttribLocation(shaderProgram, 'aTriCorner')
  let centerOffsetAttrib = gl.getAttribLocation(shaderProgram, 'aCenterOffset')
  gl.enableVertexAttribArray(texCoordAttrib)
  gl.enableVertexAttribArray(triCornerAttrib)
  gl.enableVertexAttribArray(centerOffsetAttrib)

  let sdf = wasm.march()
  let sdfTexture = createDataTexture(gl, sdf)

  let state: State = {
    wasm,
    render: {
      sdfUni,
      sdf,
      sdfTexture,
      firePosUni,
      viewUni,
      texCoordAttrib,
      triCornerAttrib,
      centerOffsetAttrib,

      xRotation: 0,
      yRotation: 0,
      imageIsLoaded: false,
      numParticles: 10500,
      redFirePos: [0.0, 0.0, 0.0],
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

  {
    let { triCorners, centerOffsets, vertexIndices, texCoords } =
      createVertexIndices(state.render.numParticles, state.render.sdf)
    function createBuffer(
      bufferType: 'ARRAY_BUFFER' | 'ELEMENT_ARRAY_BUFFER',
      DataType: any,
      data: any,
    ) {
      if (!gl) {
        throw 'todo'
      }
      var buffer = gl.createBuffer()
      gl.bindBuffer(gl[bufferType], buffer)
      gl.bufferData(gl[bufferType], new DataType(data), gl.STATIC_DRAW)
      return buffer
    }

    createBuffer('ARRAY_BUFFER', Float32Array, texCoords)
    gl.vertexAttribPointer(texCoordAttrib, 2, gl.FLOAT, false, 0, 0)

    createBuffer('ARRAY_BUFFER', Float32Array, triCorners)
    gl.vertexAttribPointer(triCornerAttrib, 2, gl.FLOAT, false, 0, 0)

    createBuffer('ARRAY_BUFFER', Float32Array, centerOffsets)
    gl.vertexAttribPointer(centerOffsetAttrib, 3, gl.FLOAT, false, 0, 0)

    createBuffer('ELEMENT_ARRAY_BUFFER', Uint16Array, vertexIndices)

    // We set OpenGL's blend function so that we don't see the black background
    // on our particle squares. Essentially, if there is anything behind the particle
    // we show whatever is behind it plus the color of the particle.
    //
    // If the color of the particle is black then black is (0, 0, 0) so we only show
    // whatever is behind it.
    // So this works because our texture has a black background.
    // There are many different blend functions that you can use, this one works for our
    // purposes.
    gl.enable(gl.DEPTH_TEST)
    gl.depthFunc(gl.LESS)
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

    // Send our perspective matrix to the GPU
    gl.uniformMatrix4fv(
      perspectiveUni,
      false,
      mat4.perspective([] as any as mat4, Math.PI / 3, 1.333, 0.01, 100),
    )
  }

  draw(state, gl)
}

//
// Initialize a shader program, so WebGL knows how to draw our data
//
function initShaderProgram(
  gl: WebGLRenderingContext,
  vsSource: string,
  fsSource: string,
) {
  let vertexShader = loadShader(gl, gl.VERTEX_SHADER, vsSource)
  let fragmentShader = loadShader(gl, gl.FRAGMENT_SHADER, fsSource)

  let shaderProgram = gl.createProgram()
  if (!shaderProgram || !vertexShader || !fragmentShader) {
    throw 'todo'
  }
  gl.attachShader(shaderProgram, vertexShader)
  gl.attachShader(shaderProgram, fragmentShader)
  gl.linkProgram(shaderProgram)

  // If creating the shader program failed, alert

  if (!gl.getProgramParameter(shaderProgram, gl.LINK_STATUS)) {
    alert(
      'Unable to initialize the shader program: ' +
        gl.getProgramInfoLog(shaderProgram),
    )
    return null
  }

  return shaderProgram
}

//
// creates a shader of the given type, uploads the source and
// compiles it.
//
function loadShader(gl: WebGLRenderingContext, type: GLenum, source: string) {
  let shader = gl.createShader(type)

  if (!shader) {
    throw 'todo'
  }

  // Send the source to the shader object

  gl.shaderSource(shader, source)

  // Compile the shader program

  gl.compileShader(shader)

  // See if it compiled successfully

  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    alert(
      'An error occurred compiling the shaders: ' + gl.getShaderInfoLog(shader),
    )
    gl.deleteShader(shader)
    return null
  }

  return shader
}

function readSdf(sdf: Float32Array, x: number, y: number, z: number) {
  let size = 128
  let depth = 32
  let x_index = x
  let y_index = y * size
  let z_index = z * size * size
  let index = x_index + y_index + z_index
  return sdf[index]
}

function readMarch(
  sdf: Float32Array,
  size: number,
  x: number,
  y: number,
  z: number,
): number {
  let x_index = x
  let y_index = y * size
  let z_index = z * size * size
  let index = x_index + y_index + z_index
  return sdf[index]
}

function createVertexIndices(numParticles: number, sdf: Float32Array) {
  let triCorners = []
  let texCoords = []
  let vertexIndices: number[][] = []
  let centerOffsets = []

  let triCornersCycle = [-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0]
  let texCoordsCycle = [0, 0, 1, 0, 1, 1, 0, 1]

  let i = 0
  let size = 64
  for (let x = 0; x < size; x++) {
    for (let y = 0; y < size; y++) {
      let z = readMarch(sdf, size, x, y, 0)
      if (z > 0.0) {
        for (let j = 0; j < 4; j++) {
          triCorners.push(triCornersCycle[j * 2])
          triCorners.push(triCornersCycle[j * 2 + 1])

          texCoords.push(texCoordsCycle[j * 2])
          texCoords.push(texCoordsCycle[j * 2 + 1])

          centerOffsets.push(x / size)
          centerOffsets.push(y / size)
          centerOffsets.push(z / 8)
        }

        vertexIndices = vertexIndices.concat(
          [0, 1, 2, 0, 2, 3].map(function (num) {
            return num + 4 * i
          }),
        )
      }
      i++
    }
  }

  // for (let i = 0; i < numParticles; i++) {
  //   for (let j = 0; j < 4; j++) {
  //     triCorners.push(triCornersCycle[j * 2])
  //     triCorners.push(triCornersCycle[j * 2 + 1])

  //     texCoords.push(texCoordsCycle[j * 2])
  //     texCoords.push(texCoordsCycle[j * 2 + 1])

  //     let x = 0.01 + Math.random() - Math.random()
  //     let y = 0.01 + Math.random() - Math.random()
  //     let z = 0.01 + Math.random() - Math.random()

  //     centerOffsets.push(x)
  //     centerOffsets.push(y)
  //     centerOffsets.push(z)
  //   }

  //   vertexIndices = vertexIndices.concat(
  //     [0, 1, 2, 0, 2, 3].map(function (num) {
  //       return num + 4 * i
  //     }),
  //   )
  // }

  return {
    texCoords,
    vertexIndices,
    centerOffsets,
    triCorners,
  }
}

function createCameraUniform(
  xRotation: number,
  yRotation: number,
  redFirePos: ReadonlyVec3,
) {
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
  mat4.lookAt(camera, cameraPos, redFirePos, [0, 1, 0])

  return camera
}

interface State {
  wasm: WasmModule
  render: RenderState
}

interface GameState {}

interface RenderState {
  sdf: Float32Array
  sdfTexture: WebGLBuffer
  shader: WebGLProgram
  viewUni: WebGLUniformLocation
  sdfUni: WebGLUniformLocation
  firePosUni: WebGLUniformLocation
  texCoordAttrib: number
  triCornerAttrib: number
  centerOffsetAttrib: number
  redFirePos: vec3
  xRotation: number
  yRotation: number
  imageIsLoaded: boolean
  numParticles: number
  lastMouseX: number
  lastMouseY: number
}

function draw(state: State, gl: WebGLRenderingContext) {
  let render = state.render

  // {
  //   let xSpeed = 0
  //   let ySpeed = 0.4
  //   state.render.xRotation += xSpeed / 50
  //   state.render.yRotation -= ySpeed / 50

  //   state.render.xRotation = Math.min(state.render.xRotation, Math.PI / 2.5)
  //   state.render.xRotation = Math.max(state.render.xRotation, -Math.PI / 2.5)
  // }

  // Once the image is loaded we'll start drawing our particle effect
  if (render.imageIsLoaded) {
    // Clear our color buffer and depth buffer so that
    // nothing is left over in our drawing buffer now that we're
    // completely redrawing the entire canvas
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT)

    gl.useProgram(render.shader)

    // Pass our world view matrix into our vertex shader
    gl.uniformMatrix4fv(
      render.viewUni,
      false,
      createCameraUniform(
        render.xRotation,
        render.yRotation,
        render.redFirePos,
      ),
    )

    // We pass information specific to our first flame into our vertex shader
    // and then draw our first flame.
    gl.uniform3fv(render.firePosUni, render.redFirePos)
    // What does numParticles * 6 mean?
    //  For each particle there are two triangles drawn (to form the square)
    //  The first triangle has 3 vertices and the second triangle has 3 vertices
    //  making for a total of 6 vertices per particle.
    // gl.drawElements(gl.TRIANGLES, render.numParticles * 6, gl.UNSIGNED_SHORT, 0)

    gl.drawArrays(gl.POINTS, 0, render.numParticles)

    // We pass information specific to our second flame into our vertex shader
    // and then draw our second flame.
    // gl.uniform3fv(render.firePosUni, render.purpFirePos)
    // gl.uniform4fv(render.colorUni, purpFireColor)
    // gl.drawElements(gl.TRIANGLES, numParticles * 6, gl.UNSIGNED_SHORT, 0)
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
    throw 'todo'
  }
  gl.bindTexture(gl.TEXTURE_2D, texture)
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.FLOAT, data)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST)
  return texture
}

function aperture<T>(n: number, arr: T[]): T[][] {
  return n > arr.length
    ? []
    : arr.slice(n - 1).map((v, i) => arr.slice(i, i + n))
}
