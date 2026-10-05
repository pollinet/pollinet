//
//  Base58.swift
//  Minimal base58 (Bitcoin alphabet) encoder for Solana pubkeys.
//

import Foundation

enum Base58 {
    private static let alphabet = [UInt8]("123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz".utf8)

    static func encode(_ bytes: [UInt8]) -> String {
        var zerosCount = 0
        while zerosCount < bytes.count && bytes[zerosCount] == 0 { zerosCount += 1 }

        var digits: [UInt8] = []
        for byte in bytes {
            var carry = Int(byte)
            for index in 0..<digits.count {
                carry += Int(digits[index]) << 8
                digits[index] = UInt8(carry % 58)
                carry /= 58
            }
            while carry > 0 {
                digits.append(UInt8(carry % 58))
                carry /= 58
            }
        }

        var result = [UInt8](repeating: alphabet[0], count: zerosCount)
        result.append(contentsOf: digits.reversed().map { alphabet[Int($0)] })
        return String(bytes: result, encoding: .utf8) ?? ""
    }
}
