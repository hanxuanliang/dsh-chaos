const ACCEPTED_TYPES = new Set(['image/png', 'image/jpeg', 'image/webp'])
const MAX_INPUT_BYTES = 8 * 1024 * 1024
const MAX_STORED_BYTES = 256 * 1024
const MAX_EDGE = 256

export const AVATAR_ACCEPT = 'image/png,image/jpeg,image/webp'

function canvasBlob(canvas: HTMLCanvasElement, quality: number): Promise<Blob> {
  return new Promise((resolve, reject) => {
    canvas.toBlob(blob => {
      if (blob === null) reject(new Error('encode'))
      else resolve(blob)
    }, 'image/webp', quality)
  })
}

function dataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onerror = () => { reject(new Error('read')) }
    reader.onload = () => {
      if (typeof reader.result === 'string') resolve(reader.result)
      else reject(new Error('read'))
    }
    reader.readAsDataURL(blob)
  })
}

/** Normalize user-selected images into a bounded local WebP payload. */
export async function normalizeAvatarFile(file: File): Promise<string> {
  if (!ACCEPTED_TYPES.has(file.type)) throw new Error('type')
  if (file.size === 0 || file.size > MAX_INPUT_BYTES) throw new Error('size')

  let bitmap: ImageBitmap
  try {
    bitmap = await createImageBitmap(file, { imageOrientation: 'from-image' })
  } catch {
    throw new Error('decode')
  }
  try {
    const scale = Math.min(1, MAX_EDGE / Math.max(bitmap.width, bitmap.height))
    const canvas = document.createElement('canvas')
    canvas.width = Math.max(1, Math.round(bitmap.width * scale))
    canvas.height = Math.max(1, Math.round(bitmap.height * scale))
    const context = canvas.getContext('2d')
    if (context === null) throw new Error('encode')
    context.drawImage(bitmap, 0, 0, canvas.width, canvas.height)
    for (const quality of [0.86, 0.72, 0.58]) {
      const blob = await canvasBlob(canvas, quality)
      if (blob.size <= MAX_STORED_BYTES) return await dataUrl(blob)
    }
    throw new Error('size')
  } finally {
    bitmap.close()
  }
}
