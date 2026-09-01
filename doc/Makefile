man:
	for man in $(shell find . -maxdepth 1 -name "*.rst" | sed -e "s@.rst@@"); do \
		rst2man $$man.rst $$man.8 ;\
	done

.PHONY: user_guide
user_guide:
	${MAKE} -C user_guide html

clean:
	rm -f *.8
	${MAKE} -C user_guide clean
