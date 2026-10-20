// Native full-song playback for Encore through MusicKit's ApplicationMusicPlayer, exposed to
// Rust as C functions. Built into libencore_native.dylib by build.rs and loaded at runtime, so
// Encore falls back to the web player where this can't load (macOS 13, `cargo run`).
//
// All functions are called on the main thread.

import AppKit
import AudioToolbox
import CoreAudio
import Darwin
import Foundation
import MusicKit

// MARK: - Tokens

/// Hands MusicKit the developer token Encore signs with the user's own MusicKit key. Without
/// it, MusicKit only works for apps registered with Apple's MusicKit service.
final class FixedTokenProvider: MusicUserTokenProvider, MusicDeveloperTokenProvider, @unchecked Sendable {
    let token: String
    init(token: String) {
        self.token = token
        super.init()
    }
    func developerToken(options: MusicTokenRequestOptions) async throws -> String { token }
}

enum VolumeControl: Int32 {
    case none = 0, requesting = 1, active = 2, unavailable = 3
}

@available(macOS 14.0, *)
@MainActor
final class Bridge {
    static let shared = Bridge()

    var player: ApplicationMusicPlayer { .shared }
    var requestedSeq: UInt64 = 0
    var loadedSeq: UInt64 = 0
    var errorSeq: UInt64 = 0
    var error = ""
    var duration: Double = 0
    var volume = VolumeControl.none
    /// Volume control was asked for; set before the first tap so the permission prompt only
    /// appears when the user wants it.
    var wantsOutput = false
    let output = GainOutput()

    func load(id: String, seq: UInt64) {
        requestedSeq = seq
        Task { @MainActor in
            do {
                let song = try await self.song(id: id)
                guard self.requestedSeq == seq else { return }
                self.duration = song.duration ?? 0
                self.player.queue = [song]
                try await self.player.play()
                guard self.requestedSeq == seq else { return }
                self.loadedSeq = seq
                self.ensureOutput()
            } catch {
                guard self.requestedSeq == seq else { return }
                self.error = "\(error)"
                self.errorSeq = seq
            }
        }
    }

    func song(id: String) async throws -> Song {
        if id.hasPrefix("i.") {
            var request = MusicLibraryRequest<Song>()
            request.filter(matching: \.id, equalTo: MusicItemID(id))
            if let song = try await request.response().items.first { return song }
        } else {
            let response = try await MusicCatalogResourceRequest<Song>(matching: \.id, equalTo: MusicItemID(id)).response()
            if let song = response.items.first { return song }
        }
        throw NSError(domain: "Encore", code: 1, userInfo: [NSLocalizedDescriptionKey: "song \(id) not found"])
    }

    /// Routes the player's audio through our own output so its volume can change. Creating the
    /// tap asks for System Audio Recording permission the first time, and blocks until the user
    /// answers, so it runs off the main thread. It waits for audio to be playing: a tap made
    /// before RemotePlayerService starts its output can stay silent.
    func ensureOutput() {
        startWatchdog()
        guard wantsOutput, volume == .none, player.state.playbackStatus == .playing, let pid = remotePlayerPID()
        else { return }
        volume = .requesting
        tapPID = pid
        silentChecks = 0
        let output = self.output
        DispatchQueue.global(qos: .userInitiated).async {
            let result = output.start(pid: pid)
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    if let result {
                        NSLog("Encore: native volume control unavailable: %@", result)
                        Bridge.shared.volume = .unavailable
                    } else {
                        Bridge.shared.volume = .active
                    }
                }
            }
        }
    }

    var tapPID: pid_t = 0
    var silentChecks = 0
    var restarts = 0
    var watchdog: Timer?

    /// The tap mutes MusicKit's own output, so if it ever stops delivering audio the user hears
    /// nothing. Once a second: start the tap when playback begins, rebuild it if
    /// RemotePlayerService restarted or the tap has gone silent while playing, and after
    /// repeated failures remove it so MusicKit's output plays directly.
    func startWatchdog() {
        guard watchdog == nil else { return }
        watchdog = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { _ in
            MainActor.assumeIsolated { Bridge.shared.checkOutput() }
        }
    }

    func checkOutput() {
        guard wantsOutput else { return }
        let playing = player.state.playbackStatus == .playing
        switch volume {
        case .none:
            ensureOutput()
        case .active:
            let (frames, peak) = output.takeInputStats()
            let pid = remotePlayerPID()
            if let pid, pid != tapPID {
                restartOutput(reason: "RemotePlayerService restarted")
            } else if playing && (frames == 0 || peak == 0) {
                silentChecks += 1
                if silentChecks >= 2 { restartOutput(reason: "no audio from the tap") }
            } else if playing {
                silentChecks = 0
                restarts = 0
            }
        case .requesting, .unavailable:
            break
        }
    }

    func restartOutput(reason: String) {
        output.stop()
        restarts += 1
        if restarts > 3 {
            NSLog("Encore: giving up on native volume control (%@)", reason)
            volume = .unavailable
            return
        }
        NSLog("Encore: rebuilding the audio tap (%@)", reason)
        volume = .none
        ensureOutput()
    }

    /// 0 stopped, 1 playing, 2 paused, 3 interrupted, 4 seeking.
    var status: Int32 {
        switch player.state.playbackStatus {
        case .stopped: return 0
        case .playing: return 1
        case .paused: return 2
        case .interrupted: return 3
        case .seekingForward, .seekingBackward: return 4
        @unknown default: return 0
        }
    }
}

// MARK: - C interface

@_cdecl("encore_native_init")
public func encoreNativeInit(_ token: UnsafePointer<CChar>) -> Bool {
    guard #available(macOS 14.0, *), Bundle.main.bundleIdentifier != nil else { return false }
    MusicDataRequest.tokenProvider = FixedTokenProvider(token: String(cString: token))
    return true
}

/// 0 not determined, 1 denied, 2 restricted, 3 authorized.
@_cdecl("encore_native_authorization")
public func encoreNativeAuthorization() -> Int32 {
    guard #available(macOS 14.0, *) else { return 2 }
    switch MusicAuthorization.currentStatus {
    case .notDetermined: return 0
    case .denied: return 1
    case .restricted: return 2
    case .authorized: return 3
    @unknown default: return 2
    }
}

@_cdecl("encore_native_request_authorization")
public func encoreNativeRequestAuthorization() {
    guard #available(macOS 14.0, *) else { return }
    Task { _ = await MusicAuthorization.request() }
}

@_cdecl("encore_native_load")
public func encoreNativeLoad(_ id: UnsafePointer<CChar>, _ seq: UInt64) {
    guard #available(macOS 14.0, *) else { return }
    let id = String(cString: id)
    MainActor.assumeIsolated { Bridge.shared.load(id: id, seq: seq) }
}

@_cdecl("encore_native_play")
public func encoreNativePlay() {
    guard #available(macOS 14.0, *) else { return }
    Task { @MainActor in try? await Bridge.shared.player.play() }
}

@_cdecl("encore_native_pause")
public func encoreNativePause() {
    guard #available(macOS 14.0, *) else { return }
    MainActor.assumeIsolated { Bridge.shared.player.pause() }
}

@_cdecl("encore_native_stop")
public func encoreNativeStop() {
    guard #available(macOS 14.0, *) else { return }
    MainActor.assumeIsolated {
        // A load still in flight shouldn't start playing after this.
        Bridge.shared.requestedSeq = 0
        Bridge.shared.player.stop()
    }
}

@_cdecl("encore_native_seek")
public func encoreNativeSeek(_ seconds: Double) {
    guard #available(macOS 14.0, *) else { return }
    MainActor.assumeIsolated { Bridge.shared.player.playbackTime = seconds }
}

/// Takes effect once volume control is active (see `encore_native_enable_volume`).
@_cdecl("encore_native_set_volume")
public func encoreNativeSetVolume(_ volume: Float) {
    guard #available(macOS 14.0, *) else { return }
    MainActor.assumeIsolated { Bridge.shared.output.gain = max(0, min(1, volume)) }
}

/// Starts routing audio through a process tap, which asks for System Audio Recording
/// permission the first time.
@_cdecl("encore_native_enable_volume")
public func encoreNativeEnableVolume() {
    guard #available(macOS 14.0, *) else { return }
    MainActor.assumeIsolated {
        Bridge.shared.wantsOutput = true
        Bridge.shared.ensureOutput()
    }
}

/// Copies the latest `count` mono samples of what's playing; returns the sample rate, or 0
/// when volume control (and so the tap) isn't active.
@_cdecl("encore_native_samples")
public func encoreNativeSamples(_ out: UnsafeMutablePointer<Float>, _ count: Int) -> Double {
    guard #available(macOS 14.0, *) else { return 0 }
    return MainActor.assumeIsolated {
        guard Bridge.shared.volume == .active else { return 0 }
        Bridge.shared.output.copySamples(into: out, count: count)
        return Bridge.shared.output.sampleRate
    }
}

/// The fallback when volume control is unavailable: the Mac's output volume.
@_cdecl("encore_native_set_system_volume")
public func encoreNativeSetSystemVolume(_ volume: Float) {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain)
    var device = AudioObjectID(kAudioObjectUnknown)
    var size = UInt32(MemoryLayout<AudioObjectID>.size)
    guard AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &device) == noErr
    else { return }
    var value = max(0, min(1, volume))
    address.mSelector = kAudioHardwareServiceDeviceProperty_VirtualMainVolume
    address.mScope = kAudioDevicePropertyScopeOutput
    AudioObjectSetPropertyData(device, &address, 0, nil, UInt32(MemoryLayout<Float32>.size), &value)
}

@_cdecl("encore_native_status")
public func encoreNativeStatus() -> Int32 {
    guard #available(macOS 14.0, *) else { return 0 }
    return MainActor.assumeIsolated { Bridge.shared.status }
}

@_cdecl("encore_native_time")
public func encoreNativeTime() -> Double {
    guard #available(macOS 14.0, *) else { return 0 }
    return MainActor.assumeIsolated { Bridge.shared.player.playbackTime }
}

@_cdecl("encore_native_duration")
public func encoreNativeDuration() -> Double {
    guard #available(macOS 14.0, *) else { return 0 }
    return MainActor.assumeIsolated { Bridge.shared.duration }
}

/// The last `load` whose song started; a load that failed shows up in `error_seq` instead.
@_cdecl("encore_native_loaded_seq")
public func encoreNativeLoadedSeq() -> UInt64 {
    guard #available(macOS 14.0, *) else { return 0 }
    return MainActor.assumeIsolated { Bridge.shared.loadedSeq }
}

@_cdecl("encore_native_error_seq")
public func encoreNativeErrorSeq() -> UInt64 {
    guard #available(macOS 14.0, *) else { return 0 }
    return MainActor.assumeIsolated { Bridge.shared.errorSeq }
}

/// 0 none yet, 1 asking for permission, 2 active, 3 unavailable (denied or failed).
@_cdecl("encore_native_volume_control")
public func encoreNativeVolumeControl() -> Int32 {
    guard #available(macOS 14.0, *) else { return 3 }
    return MainActor.assumeIsolated { Bridge.shared.volume.rawValue }
}

/// Copies the last load error into `buffer`; returns its full length in bytes.
@_cdecl("encore_native_error")
public func encoreNativeError(_ buffer: UnsafeMutablePointer<CChar>, _ capacity: Int) -> Int {
    guard #available(macOS 14.0, *) else { return 0 }
    let bytes = Array(MainActor.assumeIsolated { Bridge.shared.error }.utf8)
    let n = min(bytes.count, max(capacity - 1, 0))
    for i in 0..<n { buffer[i] = CChar(bitPattern: bytes[i]) }
    if capacity > 0 { buffer[n] = 0 }
    return bytes.count
}

// MARK: - Volume through a process tap

/// MusicKit plays through MediaPlayer's RemotePlayerService, which macOS starts for this app.
func remotePlayerPID() -> pid_t? {
    typealias Responsible = @convention(c) (pid_t) -> pid_t
    let responsible = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "responsibility_get_pid_responsible_for_pid")
        .map { unsafeBitCast($0, to: Responsible.self) }
    let count = proc_listallpids(nil, 0)
    var pids = [pid_t](repeating: 0, count: Int(count) * 2)
    let got = proc_listallpids(&pids, Int32(pids.count * MemoryLayout<pid_t>.size))
    var name = [CChar](repeating: 0, count: 256)
    var fallback: pid_t?
    for pid in pids.prefix(Int(got)) where pid > 0 {
        guard proc_name(pid, &name, UInt32(name.count)) > 0, String(cString: name) == "RemotePlayerService" else { continue }
        if responsible?(pid) == getpid() { return pid }
        fallback = max(fallback ?? 0, pid)
    }
    return fallback
}

func audioObject(for pid: pid_t) -> AudioObjectID? {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyTranslatePIDToProcessObject,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain)
    var pid = pid
    var object = AudioObjectID(kAudioObjectUnknown)
    var size = UInt32(MemoryLayout<AudioObjectID>.size)
    let status = AudioObjectGetPropertyData(
        AudioObjectID(kAudioObjectSystemObject), &address, UInt32(MemoryLayout<pid_t>.size), &pid, &size, &object)
    return status == noErr && object != kAudioObjectUnknown ? object : nil
}

func defaultOutputUID() -> String? {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain)
    var device = AudioObjectID(kAudioObjectUnknown)
    var size = UInt32(MemoryLayout<AudioObjectID>.size)
    guard AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &device) == noErr
    else { return nil }
    address.mSelector = kAudioDevicePropertyDeviceUID
    var uid: Unmanaged<CFString>?
    size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
    guard AudioObjectGetPropertyData(device, &address, 0, nil, &size, &uid) == noErr, let uid else { return nil }
    return uid.takeRetainedValue() as String
}

/// Mutes RemotePlayerService's own output with a process tap and plays the tapped audio
/// through a private aggregate device, scaled by `gain`.
final class GainOutput: @unchecked Sendable {
    /// The latest mono samples, for the visualizer. Written on the audio thread and read on
    /// the main thread without a lock: a torn read only means one slightly wrong frame of bars.
    static let ringSize = 4096
    let ring = UnsafeMutablePointer<Float>.allocate(capacity: GainOutput.ringSize)
    let ringWrite = UnsafeMutablePointer<Int>.allocate(capacity: 1)
    private(set) var sampleRate: Double = 48_000
    /// Frames and peak level received since `takeInputStats` last ran (audio thread writes).
    private let inputFrames = UnsafeMutablePointer<Int>.allocate(capacity: 1)
    private let inputPeakBits = UnsafeMutablePointer<UInt32>.allocate(capacity: 1)
    private let gainBits = UnsafeMutablePointer<UInt32>.allocate(capacity: 1)
    private var tapID = AudioObjectID(kAudioObjectUnknown)
    private var aggregateID = AudioObjectID(kAudioObjectUnknown)
    private var procID: AudioDeviceIOProcID?

    init() {
        gainBits.pointee = Float(1).bitPattern
        ring.initialize(repeating: 0, count: GainOutput.ringSize)
        ringWrite.pointee = 0
        inputFrames.pointee = 0
        inputPeakBits.pointee = 0
    }

    func takeInputStats() -> (frames: Int, peak: Float) {
        let stats = (inputFrames.pointee, Float(bitPattern: inputPeakBits.pointee))
        inputFrames.pointee = 0
        inputPeakBits.pointee = 0
        return stats
    }

    /// Copies the last `count` samples, oldest first.
    func copySamples(into out: UnsafeMutablePointer<Float>, count: Int) {
        let n = min(count, GainOutput.ringSize)
        let end = ringWrite.pointee
        for i in 0..<n {
            out[i] = ring[(end - n + i + GainOutput.ringSize * 2) % GainOutput.ringSize]
        }
    }

    var gain: Float {
        get { Float(bitPattern: gainBits.pointee) }
        set { gainBits.pointee = newValue.bitPattern }
    }

    /// Returns an error description, or nil once audio is flowing through us.
    func start(pid: pid_t) -> String? {
        guard #available(macOS 14.2, *) else { return "needs macOS 14.2" }
        guard let process = audioObject(for: pid) else { return "no audio object for RemotePlayerService" }
        guard let outputUID = defaultOutputUID() else { return "no default output device" }
        let description = CATapDescription(stereoMixdownOfProcesses: [process])
        description.uuid = UUID()
        description.muteBehavior = .muted
        description.isPrivate = true
        var status = AudioHardwareCreateProcessTap(description, &tapID)
        guard status == noErr else { return "creating the tap failed (\(status))" }
        let aggregate: [String: Any] = [
            kAudioAggregateDeviceNameKey: "Encore",
            kAudioAggregateDeviceUIDKey: UUID().uuidString,
            kAudioAggregateDeviceMainSubDeviceKey: outputUID,
            kAudioAggregateDeviceIsPrivateKey: true,
            kAudioAggregateDeviceIsStackedKey: false,
            kAudioAggregateDeviceTapAutoStartKey: true,
            kAudioAggregateDeviceSubDeviceListKey: [[kAudioSubDeviceUIDKey: outputUID]],
            kAudioAggregateDeviceTapListKey: [
                [kAudioSubTapDriftCompensationKey: true, kAudioSubTapUIDKey: description.uuid.uuidString]
            ],
        ]
        status = AudioHardwareCreateAggregateDevice(aggregate as CFDictionary, &aggregateID)
        guard status == noErr else { stop(); return "creating the output device failed (\(status))" }
        var rateAddress = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyNominalSampleRate,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain)
        var rate: Float64 = 0
        var rateSize = UInt32(MemoryLayout<Float64>.size)
        if AudioObjectGetPropertyData(aggregateID, &rateAddress, 0, nil, &rateSize, &rate) == noErr, rate > 0 {
            sampleRate = rate
        }
        let gainBits = self.gainBits
        let ring = self.ring
        let ringWrite = self.ringWrite
        let inputFrames = self.inputFrames
        let inputPeakBits = self.inputPeakBits
        status = AudioDeviceCreateIOProcIDWithBlock(&procID, aggregateID, nil) { _, input, _, output, _ in
            let gain = Float(bitPattern: gainBits.pointee)
            let ins = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: input))
            let outs = UnsafeMutableAudioBufferListPointer(output)
            Self.render(ins, into: outs, gain: gain)
            let (frames, peak) = Self.record(ins, ring: ring, write: ringWrite)
            inputFrames.pointee += frames
            inputPeakBits.pointee = max(Float(bitPattern: inputPeakBits.pointee), peak).bitPattern
        }
        guard status == noErr, let procID else { stop(); return "creating the audio callback failed (\(status))" }
        status = AudioDeviceStart(aggregateID, procID)
        guard status == noErr else { stop(); return "starting output failed (\(status))" }
        return nil
    }

    /// Appends a mono mix to the ring; returns the frame count and peak level.
    private static func record(
        _ ins: UnsafeMutableAudioBufferListPointer, ring: UnsafeMutablePointer<Float>, write: UnsafeMutablePointer<Int>
    ) -> (Int, Float) {
        guard let first = ins.first, let data = first.mData else { return (0, 0) }
        let channels = ins.count > 1 ? ins.count : max(Int(first.mNumberChannels), 1)
        let frames = Int(first.mDataByteSize) / MemoryLayout<Float>.size / (ins.count > 1 ? 1 : channels)
        let interleaved = data.assumingMemoryBound(to: Float.self)
        var w = write.pointee
        var peak: Float = 0
        for f in 0..<frames {
            var sum: Float = 0
            for c in 0..<channels {
                if ins.count > 1, let d = ins[c].mData {
                    sum += d.assumingMemoryBound(to: Float.self)[f]
                } else {
                    sum += interleaved[f * channels + c]
                }
            }
            let mono = sum / Float(channels)
            peak = max(peak, abs(mono))
            ring[w] = mono
            w = (w + 1) % GainOutput.ringSize
        }
        write.pointee = w
        return (frames, peak)
    }

    /// Copies the tapped stereo audio into whatever layout the output device uses.
    private static func render(
        _ ins: UnsafeMutableAudioBufferListPointer, into outs: UnsafeMutableAudioBufferListPointer, gain: Float
    ) {
        guard let first = ins.first, let inData = first.mData else {
            for out in outs { if let d = out.mData { memset(d, 0, Int(out.mDataByteSize)) } }
            return
        }
        let inChannels = max(Int(first.mNumberChannels), 1)
        let inFrames = Int(first.mDataByteSize) / MemoryLayout<Float>.size / (ins.count > 1 ? 1 : inChannels)
        let interleaved = inData.assumingMemoryBound(to: Float.self)
        func sample(_ frame: Int, _ channel: Int) -> Float {
            if ins.count > 1, let data = ins[min(channel, ins.count - 1)].mData {
                return data.assumingMemoryBound(to: Float.self)[frame]
            }
            return interleaved[frame * inChannels + min(channel, inChannels - 1)]
        }
        for (index, out) in outs.enumerated() {
            guard let outData = out.mData else { continue }
            let channels = max(Int(out.mNumberChannels), 1)
            let frames = Int(out.mDataByteSize) / MemoryLayout<Float>.size / channels
            let dst = outData.assumingMemoryBound(to: Float.self)
            for f in 0..<frames {
                for c in 0..<channels {
                    let channel = outs.count > 1 ? index : c
                    dst[f * channels + c] = f < inFrames ? sample(f, channel) * gain : 0
                }
            }
        }
    }

    func stop() {
        if let procID {
            AudioDeviceStop(aggregateID, procID)
            AudioDeviceDestroyIOProcID(aggregateID, procID)
            self.procID = nil
        }
        if aggregateID != kAudioObjectUnknown {
            AudioHardwareDestroyAggregateDevice(aggregateID)
            aggregateID = AudioObjectID(kAudioObjectUnknown)
        }
        if tapID != kAudioObjectUnknown {
            if #available(macOS 14.2, *) { AudioHardwareDestroyProcessTap(tapID) }
            tapID = AudioObjectID(kAudioObjectUnknown)
        }
    }
}
