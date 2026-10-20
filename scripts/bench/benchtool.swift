// Measurement helper for scripts/bench.sh.
//
//   benchtool launch <window-owner> <open args…>   ms from `open` to the app's first window
//   benchtool stats <pid>                          footprint bytes, CPU ns, process count
//
// `stats` covers the process and every helper macOS holds it responsible for (WebKit's
// content, network and GPU processes, XPC services), the same grouping Activity Monitor uses.
import CoreGraphics
import Darwin
import Foundation

func now() -> Double { Double(DispatchTime.now().uptimeNanoseconds) / 1e6 }

func hasWindow(owner: String) -> Bool {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
    return list.contains { w in
        guard (w["kCGWindowOwnerName"] as? String) == owner, (w["kCGWindowLayer"] as? Int) == 0,
              let b = w["kCGWindowBounds"] as? [String: Any] else { return false }
        return (b["Width"] as? Double ?? 0) > 400 && (b["Height"] as? Double ?? 0) > 300
    }
}

typealias ResponsibleFn = @convention(c) (pid_t) -> pid_t
let responsible: ResponsibleFn? = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "responsibility_get_pid_responsible_for_pid")
    .map { unsafeBitCast($0, to: ResponsibleFn.self) }

func allPids() -> [pid_t] {
    let n = proc_listallpids(nil, 0)
    var pids = [pid_t](repeating: 0, count: Int(n) * 2)
    let got = proc_listallpids(&pids, Int32(pids.count * MemoryLayout<pid_t>.size))
    return Array(pids.prefix(Int(got))).filter { $0 > 0 }
}

func usage(_ pid: pid_t) -> rusage_info_v4? {
    var info = rusage_info_v4()
    let ok = withUnsafeMutablePointer(to: &info) {
        $0.withMemoryRebound(to: rusage_info_t?.self, capacity: 1) { proc_pid_rusage(pid, RUSAGE_INFO_V4, $0) }
    }
    return ok == 0 ? info : nil
}

let args = CommandLine.arguments
switch args.count > 1 ? args[1] : "" {
case "launch" where args.count >= 4:
    let owner = args[2]
    let start = now()
    let open = Process()
    open.executableURL = URL(fileURLWithPath: "/usr/bin/open")
    open.arguments = Array(args[3...])
    try open.run()
    while !hasWindow(owner: owner) {
        if !open.isRunning && open.terminationStatus != 0 { exit(1) }
        if now() - start > 60_000 { print("timeout"); exit(1) }
        usleep(5_000)
    }
    print(String(format: "%.0f", now() - start))
case "stats" where args.count == 3:
    guard let root = pid_t(args[2]) else { exit(2) }
    var timebase = mach_timebase_info_data_t()
    mach_timebase_info(&timebase)
    let group = allPids().filter { $0 == root || responsible?($0) == root }
    var footprint: UInt64 = 0, cpu: UInt64 = 0
    for pid in group {
        guard let u = usage(pid) else { continue }
        footprint += u.ri_phys_footprint
        cpu += (u.ri_user_time + u.ri_system_time) * UInt64(timebase.numer) / UInt64(timebase.denom)
    }
    print(footprint, cpu, group.count)
default:
    FileHandle.standardError.write("usage: benchtool launch <owner> <open args…> | stats <pid>\n".data(using: .utf8)!)
    exit(2)
}
