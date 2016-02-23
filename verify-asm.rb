#!/usr/bin/env ruby
# parng/verify-asm.rb
#
# Copyright (c) 2016 Mozilla Foundation
#
# A very simple static analysis to verify the memory safety of the accelerated SIMD routines.

require 'set'

ALLOWED_DIRECTIVES = Set.new %w(endmacro macro)
ALLOWED_INSTRUCTIONS = Set.new(%w(and mov movd movddup movdqa movdqu movq or pabsw paddb paddw) +
                               %w(pand pcmpgtw pinsrq pmaxsw pmovzxbw por pshufb psrlw vpandn) +
                               %w(vpminsw vpshufb vpslldq vpsrldq vpsubw xorps))
ALLOWED_MEMORY_LOCATIONS = Set.new %w(dest prev src)
ALLOWED_OPERANDS = Set.new(%w(r11 r11d xmm0 xmm1 xmm2 xmm3 xmm4 xmm5 xmm6 xmm7 xmm8 xmm9 xmm10) +
                           %w(xmm11 xmm12 xmm13 xmm14 xmm15))
ALLOWED_PUNCTUATION = Set.new([',', '%'])
CRITICAL_MACROS = [
    Set.new(%w(prolog)),
    Set.new(%w(loop_start)),
    Set.new(%w(loop_end loop_end_stride)),
    Set.new(%w(epilog))
]

file = File.open ARGV[0]
error_count = 0

error = lambda do |msg|
    STDERR.puts "#{ARGV[0]}: #{file.lineno.to_s}: #{msg}"
    error_count += 1
end

file.each_line do |line|
    break if line.include? "#begin-safe-code"
end

macro_names = Set.new
critical_macro_stage = CRITICAL_MACROS.length
file.each_line do |line|
    line = line.sub(/;.*/, '')
    tokens = line.split(/\b|(?=,)|(?<=,)/)
                 .select { |word| word =~ /\S/ }
                 .map { |word| word.sub(/^\s*/, '').sub(/\s*$/, '') }
    next if tokens.empty?

    if tokens[0] == '%'
        directive = tokens[1]
        error.call "Illegal directive: #{directive}" unless ALLOWED_DIRECTIVES.include? directive
        macro_names.add tokens[2] if directive == 'macro'
        next
    end

    if tokens[1] == ':'
        error.call "Put all labels on a separate line" unless tokens.length <= 2
        unless critical_macro_stage == CRITICAL_MACROS.length
            error.call "Expected critical macro '#{CRITICAL_MACROS[critical_macro_stage].inspect}'"
        end
        critical_macro_stage = 0
        next
    end

    instruction = tokens.shift
    if critical_macro_stage < CRITICAL_MACROS.length &&
            CRITICAL_MACROS[critical_macro_stage].include?(instruction)
        critical_macro_stage += 1
        next
    end

    unless ALLOWED_INSTRUCTIONS.include?(instruction) || macro_names.include?(instruction)
        error.call "Illegal instruction: #{instruction}"
    end

    until tokens.empty?
        token = tokens.shift
        next if ALLOWED_OPERANDS.include? token
        next if ALLOWED_PUNCTUATION.include? token
        next if token =~ /^0x[0-9a-fA-F]+$/
        next if token =~ /^\d+$/
        if token == '%'
            arg = tokens.shift
            error.call "Illegal macro argument: #{arg}" unless arg =~ /^\d$/
            next
        end
        if token == '['
            memory_location = tokens.shift
            unless ALLOWED_MEMORY_LOCATIONS.include? memory_location
                error.call "Illegal memory location: #{memory_location}"
            end
            closing_bracket = tokens.shift
            unless closing_bracket == ']'
                error.call "Illegal memory location: found token #{memory_location}"
            end
            next
        end
        error.call "Illegal operand: #{token}"
    end
end

unless critical_macro_stage == CRITICAL_MACROS.length
    error.call "Expected critical macro '#{CRITICAL_MACROS[critical_macro_stage].inspect}'"
end

if error_count > 0 then
    STDERR.puts "#{error_count} error(s)"
    exit 1
end

