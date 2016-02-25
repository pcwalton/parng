#!/usr/bin/env ruby
# parng/verify-asm.rb
#
# Copyright (c) 2016 Mozilla Foundation
#
# A very simple static analysis to verify the memory safety of the accelerated SIMD routines. This
# is not intended to be a 100% sound static analysis (although patches to that effect will be
# accepted), but rather it's designed to be sound enough to give confidence in the memory safety of
# this particular code.

require 'optparse'
require 'pp'
require 'set'

CRITICAL_MACROS = [
    Set.new(%w(prolog)),
    Set.new(%w(loop_start)),
    Set.new(%w(loop_end loop_end_stride)),
    Set.new(%w(epilog))
]

WORD = 'a-zA-Z0-9_'

def check(allowed_directives: nil,
          allowed_instructions: nil,
          allowed_memory_locations: nil,
          allowed_operands: nil,
          allowed_macro_arguments: nil,
          allowed_data_types: nil,
          directive_sigil: nil,
          comment_sigil: nil,
          macro_argument_sigil: nil,
          other_allowed_sigils: nil)
    file = File.open ARGV[0]
    $error_count = 0

    error = lambda do |msg|
        STDERR.puts "#{ARGV[0]}: #{file.lineno.to_s}: #{msg}"
        $error_count += 1
    end

    file.each_line do |line|
        break if line.include? "#begin-safe-code"
    end

    macro_names = Set.new
    critical_macro_stage = CRITICAL_MACROS.length
    file.each_line do |line|
        line = line.sub(/#{comment_sigil}.*/, '')
        tokens = line.split(
        /(?<=[^#{WORD}])(?=[#{WORD}])|(?<=[#{WORD}])(?=[^#{WORD}])|(?<=[^#{WORD}])(?=[^#{WORD}])/)
                .select { |word| word =~ /\S/ }
                .map { |word| word.sub(/^\s*/, '').sub(/\s*$/, '') }
        next if tokens.empty?

        if tokens[0] == directive_sigil
            directive = tokens[1]
            unless allowed_directives.include? directive
                error.call "Illegal directive: #{directive}"
            end
            macro_names.add tokens[2] if directive == 'macro'
            next
        end

        if tokens[1] == ':'
            error.call "Put all labels on a separate line" unless tokens.length <= 2
            unless critical_macro_stage == CRITICAL_MACROS.length
                error.call "Expected critical macro " +
                    "'#{CRITICAL_MACROS[critical_macro_stage].inspect}'"
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

        unless allowed_instructions.include?(instruction) || macro_names.include?(instruction)
            error.call "Illegal instruction: #{instruction}"
        end

        if !tokens.empty? and tokens[0] == '.'
            tokens.shift
            data_type = tokens.shift
            unless allowed_data_types.include? data_type
                error.call "Illegal data type: #{data_type}"
            end
        end

        until tokens.empty?
            token = tokens.shift
            next if allowed_operands.include? token
            next if other_allowed_sigils.include? token
            next if token == ','
            next if token =~ /^0x[0-9a-fA-F]+$/
            next if token =~ /^\d+$/
            if token == macro_argument_sigil
                arg = tokens.shift
                unless allowed_macro_arguments.include? arg
                    error.call "Illegal macro argument: #{arg}"
                end
                next
            end
            if token == '['
                memory_location = tokens.shift
                unless allowed_memory_locations.include? memory_location
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
end

arch = nil
options = OptionParser.new do |opts|
    opts.banner = "usage: verify-asm.rb [--arm|--x86_64] input.asm"
    opts.on("-a", "--arm", "Verify ARM assembly") { |v| arch = 'arm' }
    opts.on("-x", "--x86_64", "Verify x86-64 assembly") { |v| arch = 'x86_64' }
end
options.parse!

case arch
when 'arm'
    check(allowed_directives: Set.new(%w(endm macro)),
          allowed_instructions: Set.new(%w(bic ldr mov orr str vabd vabs vadd vand vbic vcgt) +
                                        %w(veor vhadd vld1 vldr vmax vmin vmov vorr vsri vst1) +
                                        %w(vstr vsub vtbl vuzp vzip)),
          allowed_memory_locations: Set.new(%w(dest prev src)),
          allowed_operands: Set.new(%w(r7 d0 d1 d2 d3 d4 d5 d6 d7 s2 s3 s5 s6 s7 s8 s9 s14 s15) +
                                    %w(q0 q1 q2 q3)),
          allowed_macro_arguments: Set.new(%w(dest_lo dest_hi)),
          allowed_data_types: Set.new(%w(8 16 32 64 i32 u8 u16 s16)),
          directive_sigil: '.',
          comment_sigil: '@',
          macro_argument_sigil: '\\',
          other_allowed_sigils: Set.new([ '=', '#', '{', '}' ]))
when 'x86_64'
    check(allowed_directives: Set.new(%w(endmacro macro)),
          allowed_instructions: Set.new(%w(and mov movd movddup movdqa movdqu movq or pabsw) +
                                        %w(paddb paddw pand pcmpgtw pinsrq pmaxsw pmovzxbw por) +
                                        %w(pshufb psrlw vpandn vpminsw vpshufb vpslldq vpsrldq) +
                                        %w(vpsubw xorps)),
          allowed_memory_locations: Set.new(%w(dest prev src)),
          allowed_operands: Set.new(%w(r11 r11d xmm0 xmm1 xmm2 xmm3 xmm4 xmm5 xmm6 xmm7 xmm8) +
                                    %w(xmm9 xmm10 xmm11 xmm12 xmm13 xmm14 xmm15)),
          allowed_data_types: Set.new(),
          allowed_macro_arguments: Set.new(%w(0 1 2 3)),
          directive_sigil: '%',
          comment_sigil: ';',
          macro_argument_sigil: '%',
          other_allowed_sigils: Set.new())
else
    STDERR.puts options.banner
    options.summarize STDERR
    abort "An architecture must be specified."
end

if $error_count > 0 then
    STDERR.puts "#{$error_count} error(s)"
    exit 1
end

