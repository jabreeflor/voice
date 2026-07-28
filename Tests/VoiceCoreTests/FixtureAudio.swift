import Foundation
import XCTest
@testable import VoiceCore

/// Test-runtime audio fixtures.
///
/// Speech is synthesised with `say`, which can emit 16 kHz mono Int16 WAV
/// directly. The samples are then decoded to Float and re-encoded through
/// `Recorder.wavData` so the bytes handed to the engine are byte-for-byte the
/// same shape the app produces from a live microphone capture.
enum FixtureAudio {

    enum Failure: Error, CustomStringConvertible {
        case sayUnavailable
        case sayFailed(status: Int32, stderr: String)
        case noOutput(String)
        case malformedWAV(String)

        var description: String {
            switch self {
            case .sayUnavailable:
                return "/usr/bin/say is not available"
            case let .sayFailed(status, stderr):
                return "`say` exited with status \(status): \(stderr)"
            case let .noOutput(path):
                return "`say` produced no file at \(path)"
            case let .malformedWAV(why):
                return "could not decode generated WAV: \(why)"
            }
        }
    }

    /// Directory holding generated fixtures for the current run.
    static let directory: URL = {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("voice-fixtures-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }()

    static func removeDirectory() {
        try? FileManager.default.removeItem(at: directory)
    }

    /// Synthesised speech as Float samples at 16 kHz mono.
    /// - Parameter rate: words per minute passed to `say -r`; nil uses the default voice rate.
    static func speechSamples(_ text: String, rate: Int? = nil) throws -> [Float] {
        let sayPath = "/usr/bin/say"
        guard FileManager.default.isExecutableFile(atPath: sayPath) else {
            throw Failure.sayUnavailable
        }

        let out = directory.appendingPathComponent("say-\(UUID().uuidString).wav")
        var args = [
            "--file-format=WAVE",
            "--data-format=LEI16@16000",
            "--channels=1",
            "-o", out.path,
        ]
        if let rate = rate { args += ["-r", String(rate)] }
        args.append(text)

        let p = Process()
        p.executableURL = URL(fileURLWithPath: sayPath)
        p.arguments = args
        let err = Pipe()
        p.standardError = err
        p.standardOutput = FileHandle.nullDevice
        try p.run()
        let errData = err.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()

        guard p.terminationStatus == 0 else {
            throw Failure.sayFailed(status: p.terminationStatus,
                                    stderr: String(data: errData, encoding: .utf8) ?? "")
        }
        guard FileManager.default.fileExists(atPath: out.path) else {
            throw Failure.noOutput(out.path)
        }

        let data = try Data(contentsOf: out)
        return try floatSamples(fromWAV: data)
    }

    /// Synthesised speech encoded exactly the way `Recorder` encodes a live capture.
    static func speechWAV(_ text: String, rate: Int? = nil) throws -> Data {
        Recorder.wavData(samples: try speechSamples(text, rate: rate))
    }

    /// Digital silence, or near-silence when `amplitude` is a small non-zero value.
    static func silenceWAV(seconds: Double, amplitude: Float = 0) -> Data {
        let count = Int(seconds * 16000)
        guard amplitude > 0 else {
            return Recorder.wavData(samples: [Float](repeating: 0, count: count))
        }
        var rng = SystemRandomNumberGenerator()
        let samples = (0..<count).map { _ in
            Float.random(in: -amplitude...amplitude, using: &rng)
        }
        return Recorder.wavData(samples: samples)
    }

    static func duration(ofSamples samples: [Float]) -> Double {
        Double(samples.count) / 16000.0
    }

    // MARK: - WAV decoding

    /// Walks the RIFF chunk list to find `data`. `say` writes a 4 KB header
    /// region, so the payload is not at the canonical offset 44.
    static func floatSamples(fromWAV data: Data) throws -> [Float] {
        func u32(_ offset: Int) throws -> UInt32 {
            guard offset + 4 <= data.count else {
                throw Failure.malformedWAV("truncated at byte \(offset)")
            }
            return data.subdata(in: offset..<(offset + 4)).withUnsafeBytes {
                $0.loadUnaligned(as: UInt32.self).littleEndian
            }
        }
        func tag(_ offset: Int) throws -> String {
            guard offset + 4 <= data.count else {
                throw Failure.malformedWAV("truncated at byte \(offset)")
            }
            return String(decoding: data.subdata(in: offset..<(offset + 4)), as: UTF8.self)
        }

        guard data.count > 12, try tag(0) == "RIFF", try tag(8) == "WAVE" else {
            throw Failure.malformedWAV("not a RIFF/WAVE file")
        }

        var cursor = 12
        var payload: Data?
        while cursor + 8 <= data.count {
            let id = try tag(cursor)
            let size = Int(try u32(cursor + 4))
            let body = cursor + 8
            if id == "data" {
                let end = min(body + size, data.count)
                guard end > body else { throw Failure.malformedWAV("empty data chunk") }
                payload = data.subdata(in: body..<end)
                break
            }
            cursor = body + size + (size % 2)  // chunks are word-aligned
        }

        guard let pcm = payload else { throw Failure.malformedWAV("no data chunk") }

        let count = pcm.count / 2
        var samples = [Float](repeating: 0, count: count)
        pcm.withUnsafeBytes { raw in
            for i in 0..<count {
                let v = raw.loadUnaligned(fromByteOffset: i * 2, as: Int16.self).littleEndian
                samples[i] = Float(v) / 32768.0
            }
        }
        return samples
    }
}
