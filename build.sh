#!/bin/bash

# Build the bootloader
echo "Building bootloader..."
nasm -f bin boot.asm -o boot.bin

if [ $? -eq 0 ]; then
    echo "Build successful!"
    echo "Running in QEMU..."
    qemu-system-x86_64 -fda boot.bin -display gtk
else
    echo "Build failed!"
    exit 1
fi 