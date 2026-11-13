// Rendert eine Seite unsichtbar in WebKit (wie Funkels Fenster) und speichert Bilder.
// Kein Fenster auf dem Bildschirm. Aufruf:
//   swift scripts/schnappschuss.swift URL BREITE HOEHE MASSSTAB ZIEL.png [MS ...]
// Mehrere Zeitpunkte (ms nach dem Laden) ergeben ZIEL-<ms>.png.
import AppKit
import WebKit

let a = CommandLine.arguments
let url = URL(string: a[1])!
let (b, h, m) = (CGFloat(Double(a[2])!), CGFloat(Double(a[3])!), CGFloat(Double(a[4])!))
let ziel = a[5]
let zeiten = a.count > 6 ? a[6...].map { Int($0)! } : [800]

final class Helfer: NSObject, WKNavigationDelegate {
    var fertig: (() -> Void)?
    func webView(_ w: WKWebView, didFinish _: WKNavigation!) { fertig?() }
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let fenster = NSWindow(contentRect: NSRect(x: -20000, y: -20000, width: b, height: h), styleMask: .borderless, backing: .buffered, defer: false)
let web = WKWebView(frame: NSRect(x: 0, y: 0, width: b, height: h))
web.setValue(false, forKey: "drawsBackground")
fenster.contentView = web
fenster.backgroundColor = .black
let helfer = Helfer()
web.navigationDelegate = helfer
helfer.fertig = {
    let start = DispatchTime.now()
    var offen = zeiten.count
    for ms in zeiten {
        DispatchQueue.main.asyncAfter(deadline: start + .milliseconds(ms)) {
            let k = WKSnapshotConfiguration()
            k.snapshotWidth = NSNumber(value: Double(b))
            web.takeSnapshot(with: k) { bild, fehler in
                if let bild, let tiff = bild.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff) {
                    rep.size = NSSize(width: b * m, height: h * m)
                    let pfad = zeiten.count > 1 ? ziel.replacingOccurrences(of: ".png", with: "-\(ms).png") : ziel
                    try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: pfad))
                } else { print("Fehler:", fehler as Any) }
                offen -= 1
                if offen == 0 { exit(0) }
            }
        }
    }
}
web.load(URLRequest(url: url))
DispatchQueue.main.asyncAfter(deadline: .now() + 30) { print("Zeitüberschreitung"); exit(1) }
app.run()
