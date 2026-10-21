// Drives a running Encore for UI checks: `swift scripts/uidrive.swift <app> <command> [args]`.
// <app> is the process name ("Encore" for the installed app, "encore" for a cargo build).
//
//   tree                     accessibility tree of the main window (role, label, value, frame)
//   press <label>            AXPress the first element whose label or value equals <label>
//   shot <path>              screenshot of the main window
//   scroll <dy> [x y]        a trackpad swipe of dy points (with began/changed/ended phases)
//   fling <dy> [x y]         a swipe followed by momentum events
//   wheel <lines> [x y]      a mouse-wheel scroll in lines
//   click <x> <y>            left click at window-relative points
//   hover <x> <y>            move the pointer to window-relative points
//   type <text>              type text as key events
//   key <name>               press return, escape, down, up, tab or space
//
// Needs Accessibility and Screen Recording permission for whatever runs it.

import AppKit
import ApplicationServices

let args = CommandLine.arguments
guard args.count >= 3 else { FileHandle.standardError.write("usage: uidrive <app> <command> [args]\n".data(using: .utf8)!); exit(2) }
let appName = args[1]
let command = args[2]

func fail(_ message: String) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(1)
}

guard let app = NSWorkspace.shared.runningApplications.first(where: {
    $0.localizedName == appName || $0.executableURL?.lastPathComponent == appName
}) else { fail("\(appName) is not running") }
let pid = app.processIdentifier

func windowInfo() -> (id: CGWindowID, bounds: CGRect) {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
    let windows = list.filter { ($0[kCGWindowOwnerPID as String] as? pid_t) == pid && ($0[kCGWindowLayer as String] as? Int) == 0 }
    guard let w = windows.max(by: { area($0) < area($1) }) else { fail("no window") }
    let b = w[kCGWindowBounds as String] as! [String: CGFloat]
    return (w[kCGWindowNumber as String] as! CGWindowID, CGRect(x: b["X"]!, y: b["Y"]!, width: b["Width"]!, height: b["Height"]!))
}

func area(_ w: [String: Any]) -> CGFloat {
    let b = w[kCGWindowBounds as String] as? [String: CGFloat] ?? [:]
    return (b["Width"] ?? 0) * (b["Height"] ?? 0)
}

func point(_ i: Int) -> CGPoint {
    let bounds = windowInfo().bounds
    if args.count > i + 1, let x = Double(args[i]), let y = Double(args[i + 1]) {
        return CGPoint(x: bounds.minX + x, y: bounds.minY + y)
    }
    return CGPoint(x: bounds.midX + 120, y: bounds.midY)
}

func attr(_ e: AXUIElement, _ name: String) -> AnyObject? {
    var value: AnyObject?
    return AXUIElementCopyAttributeValue(e, name as CFString, &value) == .success ? value : nil
}

func frame(_ e: AXUIElement) -> CGRect {
    var p = CGPoint.zero, s = CGSize.zero
    if let v = attr(e, kAXPositionAttribute) { AXValueGetValue(v as! AXValue, .cgPoint, &p) }
    if let v = attr(e, kAXSizeAttribute) { AXValueGetValue(v as! AXValue, .cgSize, &s) }
    return CGRect(origin: p, size: s)
}

func mainWindow() -> AXUIElement {
    let ax = AXUIElementCreateApplication(pid)
    guard let windows = attr(ax, kAXWindowsAttribute) as? [AXUIElement], !windows.isEmpty else {
        fail("no accessible window (is Accessibility permission granted?)")
    }
    return windows.max(by: { frame($0).width * frame($0).height < frame($1).width * frame($1).height })!
}

func walk(_ e: AXUIElement, depth: Int, seen: inout [AXUIElement], _ visit: (AXUIElement, Int) -> Bool) -> Bool {
    if depth > 40 || seen.contains(where: { CFEqual($0, e) }) { return false }
    seen.append(e)
    if visit(e, depth) { return true }
    for child in attr(e, kAXChildrenAttribute) as? [AXUIElement] ?? [] {
        if walk(child, depth: depth + 1, seen: &seen, visit) { return true }
    }
    return false
}

func walk(_ e: AXUIElement, _ visit: (AXUIElement, Int) -> Bool) -> Bool {
    var seen: [AXUIElement] = []
    return walk(e, depth: 0, seen: &seen, visit)
}

func describe(_ e: AXUIElement) -> String {
    let role = attr(e, kAXRoleAttribute) as? String ?? "?"
    let label = attr(e, kAXDescriptionAttribute) as? String ?? attr(e, kAXTitleAttribute) as? String ?? ""
    let value = attr(e, kAXValueAttribute).map { "\($0)" } ?? ""
    let f = frame(e)
    return "\(role) label=\"\(label)\" value=\"\(value)\" (\(Int(f.minX)),\(Int(f.minY)) \(Int(f.width))x\(Int(f.height)))"
}

func post(_ event: CGEvent?) {
    event?.post(tap: .cghidEventTap)
    usleep(8_000)
}

func moveTo(_ p: CGPoint) {
    post(CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left))
    usleep(40_000)
}

func scrollEvent(_ dy: Int32, phase: Int64, momentum: Int64) -> CGEvent? {
    let e = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: dy, wheel2: 0, wheel3: 0)
    e?.setIntegerValueField(.scrollWheelEventIsContinuous, value: 1)
    e?.setIntegerValueField(.scrollWheelEventScrollPhase, value: phase)
    e?.setIntegerValueField(.scrollWheelEventMomentumPhase, value: momentum)
    return e
}

// NSEvent phases: began 1, changed 4, ended 8. Momentum: begin 1, continue 2, end 3.
func swipe(_ total: Int, steps: Int = 20) {
    let step = Int32(total / steps)
    post(scrollEvent(0, phase: 1, momentum: 0))
    for _ in 0..<steps {
        post(scrollEvent(step, phase: 4, momentum: 0))
        usleep(8_000)
    }
    post(scrollEvent(0, phase: 8, momentum: 0))
}

switch command {
case "tree":
    _ = walk(mainWindow()) { e, depth in
        print(String(repeating: "  ", count: depth) + describe(e))
        return false
    }
case "press":
    guard args.count > 3 else { fail("press <label>") }
    let target = args[3]
    var found: AXUIElement?
    _ = walk(mainWindow()) { e, _ in
        let label = attr(e, kAXDescriptionAttribute) as? String ?? attr(e, kAXTitleAttribute) as? String
        let value = attr(e, kAXValueAttribute) as? String
        if label == target || value == target { found = e; return true }
        return false
    }
    guard let e = found else { fail("no element labeled \"\(target)\"") }
    let result = AXUIElementPerformAction(e, kAXPressAction as CFString)
    print(result == .success ? "pressed \(describe(e))" : "press failed (\(result.rawValue))")
case "shot":
    guard args.count > 3 else { fail("shot <path>") }
    let task = Process()
    task.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
    task.arguments = ["-x", "-o", "-l\(windowInfo().id)", args[3]]
    try task.run()
    task.waitUntilExit()
    print(task.terminationStatus == 0 ? args[3] : "screencapture failed")
case "scroll":
    moveTo(point(4))
    swipe(Int(args.count > 3 ? args[3] : "-400") ?? -400)
case "fling":
    moveTo(point(4))
    let total = Int(args.count > 3 ? args[3] : "-600") ?? -600
    swipe(total, steps: 8)
    var v = Int32(total / 8)
    post(scrollEvent(v, phase: 0, momentum: 1))
    for _ in 0..<40 {
        v = Int32(Double(v) * 0.9)
        post(scrollEvent(v, phase: 0, momentum: 2))
        usleep(8_000)
    }
    post(scrollEvent(0, phase: 0, momentum: 3))
case "wheel":
    moveTo(point(4))
    let lines = Int32(args.count > 3 ? args[3] : "-3") ?? -3
    post(CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 1, wheel1: lines, wheel2: 0, wheel3: 0))
case "click":
    let p = point(3)
    moveTo(p)
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: p, mouseButton: .left))
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: p, mouseButton: .left))
case "hover":
    moveTo(point(3))
case "type":
    guard args.count > 3 else { fail("type <text>") }
    for ch in args[3].utf16 {
        var c = ch
        for down in [true, false] {
            let e = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: down)
            e?.keyboardSetUnicodeString(stringLength: 1, unicodeString: &c)
            e?.postToPid(pid)
            usleep(12_000)
        }
    }
case "key":
    let codes: [String: CGKeyCode] = ["return": 36, "escape": 53, "down": 125, "up": 126, "tab": 48, "space": 49]
    guard args.count > 3, let code = codes[args[3]] else { fail("key return|escape|down|up|tab|space") }
    for down in [true, false] {
        CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down)?.postToPid(pid)
        usleep(12_000)
    }
default:
    fail("unknown command \(command)")
}
