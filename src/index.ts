import { render } from './render'

export type WasmModule = typeof import('../crates/game/pkg/game')

export default function startup() {}

export function start(wasm: WasmModule) {
  let canvas = createCanvas()
  render(wasm, canvas.canvas)
}

function createCanvas() {
  document.body.setAttribute('style', 'padding:0;margin:0;overflow:hidden;')

  let container = document.createElement('div')
  container.setAttribute(
    'style',
    'width:100vw;height:100vh;outline:none; display: flex; align-items: center; justify-content: center; background-color: white;',
  )
  document.body.appendChild(container)
  let { width: maxWidth, height: maxHeight } = container.getBoundingClientRect()
  let ratioWidth = 1024
  let ratioHeight = 768
  maxWidth = Math.min(maxWidth, ratioWidth)
  maxHeight = Math.min(maxHeight, ratioHeight)

  let ratio = maxWidth / ratioWidth
  if (ratioHeight * ratio > maxHeight) {
    ratio = maxHeight / ratioHeight
  }

  if (ratio > 1) {
    ratio = 1
  }

  let gameWidth = ratio * ratioWidth
  let gameHeight = ratio * ratioHeight

  let area = document.createElement('div')
  area.setAttribute(
    'style',
    `width:${gameWidth}px;height:${gameHeight}px;outline:none;background-color: #111111;`,
  )
  container.appendChild(area)
  let canvas = document.createElement('canvas')
  canvas.addEventListener(
    'webglcontextlost',
    (event) => {
      console.error('CONTEXT LOST')
    },
    false,
  )
  area.appendChild(canvas)
  canvas.setAttribute('tabIndex', `0`)
  canvas.setAttribute('width', `${gameWidth * 2}`)
  canvas.setAttribute('height', `${gameHeight * 2}`)
  canvas.setAttribute(
    'style',
    `width:${gameWidth}px;height:${gameHeight}px;outline:none;`,
  )
  return { area, canvas }
}
