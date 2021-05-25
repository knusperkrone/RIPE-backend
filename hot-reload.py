# /usr/bin/python

import os
import time
import subprocess





def reload_plugins():
    print('Compiling..')
    build = subprocess.Popen(['cargo', 'build'], stderr=subprocess.PIPE)
    stdout, stderr = build.communicate()
    output = stderr.decode("utf-8")

    # extract compiled plugins from build command
    updated_plugins = []
    for line in output.splitlines():
        needle = 'Compiling '
        index = line.find(needle)
        if index != -1:
            name_index = index + len(needle)
            plugin_name = line[name_index: line.find(' ', name_index)]
            lib_name = f'lib{plugin_name}'
            updated_plugins.append(lib_name)

    # list, filter and count existing plugins
    os.chdir('target/debug')
    libs = {}
    for entry in os.listdir('.'):
        needle ='.so'
        index = entry.find(needle)
        if index != -1:
            key = entry[0:index]
            if key in updated_plugins:
                if not key in libs:
                    libs[key] = 0
                libs[key] += 1

    for k in libs:
        orig_name = f'{k}.so'
        new_name = f'{k}.so{libs[k] + 1}'
        subprocess.Popen(['cp', orig_name, new_name])
        print('updated', k)
