.PHONY: doc

all:	doc

doc:	parng.h
	mkdir -p doc && cldoc generate -- --output doc parng.h

