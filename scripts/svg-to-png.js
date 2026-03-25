const { app, BrowserWindow } = require('electron')
const { join } = require('path')
const { writeFileSync, readFileSync } = require('fs')

app.whenReady().then(async () => {
  const win = new BrowserWindow({ width: 1024, height: 1024, show: false })

  const svg = readFileSync(join(__dirname, '../build/icon.svg'), 'utf8')
  const html = `<html><body style="margin:0;padding:0;width:1024px;height:1024px;overflow:hidden">${svg}</body></html>`
  await win.loadURL('data:text/html,' + encodeURIComponent(html))

  await new Promise(r => setTimeout(r, 500))

  const image = await win.webContents.capturePage()
  writeFileSync(join(__dirname, '../build/icon.png'), image.toPNG())
  console.log('Saved build/icon.png')
  app.quit()
})
